"""Chromium window for WOS. GUI and downloads stay in this private process."""
from __future__ import annotations

import json
import queue
import sys
import threading
import time
import uuid
from pathlib import Path
from urllib.parse import urlsplit, unquote

BASE = Path(__file__).resolve().parent
sys.path.insert(0, str(BASE / 'runtime' / 'browser-deps'))
from PySide6.QtCore import QTimer, QUrl, Qt
from PySide6.QtGui import QDesktopServices
from PySide6.QtWidgets import (QApplication, QMainWindow, QLineEdit, QLabel,
    QWidget, QVBoxLayout, QHBoxLayout, QPushButton, QProgressBar, QListWidget,
    QListWidgetItem, QSplitter)
from PySide6.QtWebEngineCore import QWebEnginePage, QWebEngineProfile, QWebEngineDownloadRequest
from PySide6.QtWebEngineWidgets import QWebEngineView

ORIGINS = ('https://webofscience.clarivate.cn', 'https://www.webofscience.com')


def origin(url):
    parsed = urlsplit(url)
    if parsed.username or parsed.password:
        return ''
    return f'{parsed.scheme}://{parsed.netloc}'


def wos_page(url):
    return origin(url) in ORIGINS and urlsplit(url).path.startswith('/wos/')


class BrowserWindow(QMainWindow):
    def __init__(self, root, emit, initial_url=None):
        super().__init__()
        self.root, self.emit = Path(root), emit
        self.download_dir = self.root / 'downloads'
        self.download_dir.mkdir(parents=True, exist_ok=True)
        self.profile = QWebEngineProfile('wos-assistant', self)
        self.profile.setPersistentStoragePath(str(self.root / 'profile'))
        self.profile.setCachePath(str(self.root / 'cache'))
        self.profile.setDownloadPath(str(self.download_dir))
        self.view = QWebEngineView(self)
        self.popups = []
        owner=self
        class Page(QWebEnginePage):
            def createWindow(self, kind):
                popup=QWebEngineView()
                popup.setWindowTitle('WOS · 机构访问')
                page=Page(owner.profile,popup)
                popup.setPage(page)
                page.windowCloseRequested.connect(popup.close)
                owner.popups.append(popup)
                popup.resize(1000,750)
                popup.show()
                return page
        self.page = Page(self.profile, self.view)
        self.view.setPage(self.page)
        self.setWindowTitle('WOS 内置浏览器 · 下载直接保存到助手目录')
        self.resize(1200, 820)
        self.setMinimumSize(900, 620)
        shell = QWidget()
        layout = QVBoxLayout(shell)
        layout.setContentsMargins(16, 14, 16, 12)
        layout.setSpacing(12)
        header = QHBoxLayout()
        title = QLabel('WOS 文献工作台')
        title.setObjectName('heading')
        header.addWidget(title)
        header.addStretch()
        self.page_state = QLabel('等待打开网页')
        self.page_state.setObjectName('badge')
        header.addWidget(self.page_state)
        self.panel_button = QPushButton('隐藏下载列表')
        header.addWidget(self.panel_button)
        layout.addLayout(header)
        navigation = QHBoxLayout()
        for label, callback in [('后退', self.view.back), ('前进', self.view.forward),
                ('刷新', self.view.reload), ('WOS 首页', lambda:self.view.setUrl(QUrl(ORIGINS[0]+'/wos/')))]:
            button = QPushButton(label)
            button.clicked.connect(callback)
            navigation.addWidget(button)
        self.address = QLineEdit(self)
        self.address.setPlaceholderText('输入网址，按 Enter 打开')
        self.address.setAccessibleName('网页地址')
        self.address.returnPressed.connect(self.navigate)
        navigation.addWidget(self.address, 1)
        layout.addLayout(navigation)
        self.view.urlChanged.connect(lambda url:self.address.setText(url.toString()))
        self.loading = QProgressBar()
        self.loading.setFixedHeight(3)
        self.loading.setTextVisible(False)
        self.loading.hide()
        layout.addWidget(self.loading)
        self.view.loadStarted.connect(lambda: self.loading.show())
        self.view.loadProgress.connect(self.loading.setValue)
        self.view.loadFinished.connect(self.page_loaded)
        splitter = QSplitter(Qt.Orientation.Horizontal)
        splitter.addWidget(self.view)
        self.download_panel = QWidget()
        side = QVBoxLayout(self.download_panel)
        side.setContentsMargins(16, 10, 4, 0)
        side_title = QLabel('本次窗口的下载')
        side_title.setObjectName('subheading')
        side.addWidget(side_title)
        hint = QLabel('先在网页中完成机构登录，再回到助手开始批量下载。')
        hint.setWordWrap(True)
        side.addWidget(hint)
        self.download_summary = QLabel('尚无下载')
        side.addWidget(self.download_summary)
        self.download_list = QListWidget()
        self.download_list.setWordWrap(True)
        side.addWidget(self.download_list, 1)
        note = QLabel('批量任务下载后会继续核验，通过后归入“待收导出”。手动下载的文件请回助手整理。')
        note.setWordWrap(True)
        side.addWidget(note)
        folder = QPushButton('打开下载目录')
        folder.setObjectName('primary')
        folder.clicked.connect(lambda: QDesktopServices.openUrl(QUrl.fromLocalFile(str(self.download_dir.resolve()))))
        side.addWidget(folder)
        path_label = QLineEdit(str(self.download_dir.resolve()))
        path_label.setReadOnly(True)
        path_label.setCursorPosition(0)
        path_label.setToolTip(str(self.download_dir.resolve()))
        path_label.setAccessibleName('下载保存目录，可复制')
        side.addWidget(path_label)
        splitter.addWidget(self.download_panel)
        splitter.setSizes([850, 300])
        splitter.setCollapsible(0, False)
        splitter.setCollapsible(1, False)
        self.download_panel.setMinimumWidth(240)
        layout.addWidget(splitter, 1)
        self.status = QLabel('请先登录 WOS；文件将直接保存到助手目录。')
        self.status.setWordWrap(True)
        self.status.setTextFormat(Qt.TextFormat.PlainText)
        self.status.setTextInteractionFlags(Qt.TextInteractionFlag.TextSelectableByMouse)
        layout.addWidget(self.status)
        self.setCentralWidget(shell)
        self.panel_button.clicked.connect(self.toggle_download_panel)
        self.setStyleSheet('''
            QMainWindow, QWidget { background: #F2F1EF; color: #262527; font: 10pt "Microsoft YaHei UI"; }
            QLabel#heading { font-size: 17pt; font-weight: 600; }
            QLabel#subheading { font-size: 12pt; font-weight: 600; }
            QLabel#badge { background: #EAE8E5; border-radius: 10px; padding: 6px 12px; }
            QPushButton { background: #FCFBF9; border: 1px solid #D9D5D1; border-radius: 6px; padding: 7px 12px; }
            QPushButton:hover { background: #F2E3E5; border-color: #982D3D; }
            QPushButton:focus, QLineEdit:focus { border: 1px solid #982D3D; }
            QPushButton#primary { background: #982D3D; color: white; border-color: #982D3D; }
            QPushButton#primary:hover { background: #AB3548; }
            QLineEdit { background: #FCFBF9; border: 1px solid #D9D5D1; border-radius: 6px; padding: 7px; }
            QListWidget { background: #FCFBF9; border: 1px solid #D9D5D1; border-radius: 8px; }
            QListWidget::item { padding: 12px 8px; border-bottom: 1px solid #EAE8E5; }
            QListWidget::item:selected { background: #F2E3E5; color: #262527; }
            QProgressBar { border: none; background: #EAE8E5; }
            QProgressBar::chunk { background: #982D3D; }
        ''')
        self.completed_downloads = 0
        self.failed_downloads = 0
        self.command = None
        self.capture = None
        self.downloads = set()
        self.adapter = (BASE / 'extension' / 'wos-adapter.js').read_text(encoding='utf-8')
        self.profile.downloadRequested.connect(self.download_requested)
        self.incoming = queue.Queue()
        self.timer = QTimer(self)
        self.timer.timeout.connect(self.tick)
        self.timer.start(100)
        if initial_url:
            self.view.setUrl(QUrl(initial_url))

    def toggle_download_panel(self):
        visible = not self.download_panel.isHidden()
        self.download_panel.setVisible(not visible)
        self.panel_button.setText('显示下载列表' if visible else '隐藏下载列表')

    def page_loaded(self, ok):
        self.loading.hide()
        self.page_state.setText('WOS 页面已打开' if ok and wos_page(self.page.url().toString())
                                else '网页已打开' if ok else '网页加载失败')

    def update_download_summary(self):
        self.download_summary.setText(
            f'下载中 {len(self.downloads)} · 已保存 {self.completed_downloads} · 失败 {self.failed_downloads}')

    def navigate(self):
        url = QUrl.fromUserInput(self.address.text())
        if url.scheme() in ('https','http'):
            self.view.setUrl(url)

    def finish(self, ok, data=None, error=''):
        command, self.command = self.command, None
        capture, self.capture = self.capture, None
        if not ok and capture and capture.get('download'):
            capture['download'].cancel()
        if command:
            self.emit({'id':command['id'], 'ok':ok, 'data':data or {}, 'error':error})
        self.status.setText(error if not ok else '文件已下载，等待助手核验并归档。'
                            if data and data.get('path') else '已定位论文记录，等待导出。')

    def tick(self):
        while not self.incoming.empty():
            message = self.incoming.get_nowait()
            if not isinstance(message, dict):
                continue
            action = message.get('action')
            if action=='close':
                self.close()
                return
            if action=='show':
                self.showNormal(); self.raise_(); self.activateWindow()
                continue
            if action=='cancel':
                if self.command and self.command['id']==message.get('id'):
                    self.finish(False, error='下载已取消。')
                continue
            if self.command:
                self.emit({'id':message.get('id'),'ok':False,'error':'已有命令正在执行。'})
                continue
            expires = message.get('expires')
            if not isinstance(expires, (int, float)) or time.time()*1000 >= expires-1500:
                self.emit({'id':message.get('id'),'ok':False,
                           'error':'WOS 命令已过期，未操作网页。'})
                continue
            self.command = message
            if action not in ('wos_search','wos_export') or not wos_page(self.page.url().toString()):
                self.finish(False, error='请先在内置浏览器中登录并打开 WOS 文献检索页。')
                continue
            self.command_origin = origin(self.page.url().toString())
            self.status.setText('正在检索…' if action=='wos_search' else '正在导出完整记录…')
            if action=='wos_search':
                self.search(False)
            else:
                self.script('wos_prepare_export', self.prepared)
        if self.command and time.time()*1000>=self.command['expires']-1500:
            self.finish(False, error='WOS 页面操作超时，未重复提交。')

    def later(self, callback, ms=250):
        ident = self.command and self.command['id']
        QTimer.singleShot(ms, lambda:callback() if self.command and self.command['id']==ident else None)

    def script(self, action, callback):
        if not self.command:
            return
        if origin(self.page.url().toString())!=self.command_origin:
            self.finish(False, error='WOS 已跳转到其他网站，请先完成机构登录。')
            return
        key = '__wosNative_'+uuid.uuid4().hex
        command = {**self.command,'action':action,'defer_click':True}
        source = self.adapter+'\n;runWOSCommand('+json.dumps(command)+').then(r=>window['+json.dumps(key)+']=r);'
        self.page.runJavaScript(source)
        def poll():
            self.page.runJavaScript('JSON.stringify(window['+json.dumps(key)+'] || null)', received)
        def received(raw):
            if not self.command or self.command['id']!=command['id']:
                return
            try:
                result = json.loads(raw or 'null')
            except (ValueError, TypeError):
                result = None
            if result is None:
                self.later(poll)
            else:
                self.page.runJavaScript('delete window['+json.dumps(key)+']')
                callback(result)
        self.later(poll, 30)

    def loaded(self, url, callback):
        ident=self.command and self.command['id']
        def done(ok):
            self.page.loadFinished.disconnect(done)
            if self.command and self.command['id']==ident:
                if ok:
                    callback()
                else:
                    self.finish(False,error='WOS 页面加载失败，请检查网络或机构登录。')
        self.page.loadFinished.connect(done)
        self.view.setUrl(QUrl(url))

    def search(self, clean):
        url = self.page.url().toString()
        if clean or not urlsplit(url).path.rstrip('/').endswith(('basic-search','advanced-search','fielded-search','/wos')):
            self.loaded(self.command_origin+'/wos/woscc/basic-search', lambda:self.start_search(True))
        else:
            self.start_search(False)

    def start_search(self, cleaned):
        def started(result):
            if not result.get('ok'):
                if '保留上一条零结果' in result.get('error','') and not cleaned:
                    self.search(True)
                else:
                    self.finish(False,error=result.get('error','检索失败'))
            else:
                command={**self.command,'action':'wos_submit_search'}
                self.page.runJavaScript(self.adapter+'\n;runWOSCommand('+json.dumps(command)+');')
                self.later(self.read_results)
        self.script('wos_start_search', started)

    def read_results(self):
        def read(result):
            if not result.get('ok'):
                self.finish(False,error=result.get('error','读取结果失败'))
                return
            data=result.get('data',{})
            state=data.get('state')
            if state=='loading':
                self.later(self.read_results,500)
            elif state=='zero':
                self.finish(False,error='WOS 未找到记录；继续下一篇。')
            elif state=='multiple':
                self.finish(False,error='WOS 结果不是可确认的唯一记录，请核对。')
            elif state=='record':
                self.finish(True,data)
            elif state=='single':
                import re
                url=data.get('navigate_url','')
                if origin(url)!=self.command_origin or not re.fullmatch(r'/wos/woscc/full-record/WOS:\d{15}/?',unquote(urlsplit(url).path)):
                    self.finish(False,error='WOS 结果链接不符合单篇记录格式。')
                else:
                    self.loaded(url,self.read_results)
            else:
                self.finish(False,error='未知 WOS 检索状态。')
        self.script('wos_read_results',read)

    def prepared(self,result):
        if not result.get('ok'):
            self.finish(False,error=result.get('error','导出准备失败'))
            return
        self.capture={'record_url':result['data']['record_url'],'download':None}
        def clicked(result):
            if not result.get('ok'):
                self.finish(False,error=result.get('error','导出失败'))
        self.script('wos_download',clicked)

    def download_requested(self, download):
        url=download.url().toString()
        source=url[5:] if url.startswith('blob:') else url
        page_url=self.page.url().toString()
        if download.page()!=self.page or not wos_page(page_url) or origin(source)!=origin(page_url) or not download.suggestedFileName().lower().endswith('.txt'):
            download.cancel()
            return
        if self.command and not self.capture:
            download.cancel()
            return
        capture=self.capture
        if capture:
            current=page_url.replace('(overlay:export/ext)','')
            if unquote(current.split('#')[0])!=unquote(capture['record_url'].split('#')[0]):
                download.cancel()
                self.finish(False,error='下载页面与本次导出记录不一致。')
                return
        if capture and capture['download']:
            download.cancel()
            self.finish(False,error='出现多个下载，未采纳。')
            return
        filename=uuid.uuid4().hex+'.txt'
        download.setDownloadDirectory(str(self.download_dir.resolve()))
        download.setDownloadFileName(filename)
        self.downloads.add(download)
        label = str((self.command or {}).get('title') or download.suggestedFileName())
        item = QListWidgetItem(f'下载中 · {label}\n等待接收文件…')
        item.setToolTip(label)
        self.download_list.insertItem(0, item)
        self.update_download_summary()
        if capture:
            capture['download']=download
        def progress():
            received, total = download.receivedBytes(), download.totalBytes()
            amount = f'{received / 1024:.1f} KB'
            if total > 0:
                amount += f' / {total / 1024:.1f} KB'
            item.setText(f'下载中 · {label}\n{amount}')
            if max(download.receivedBytes(),download.totalBytes())>524288:
                download.cancel()
        def finished():
            if not download.isFinished() or download not in self.downloads:
                return
            self.downloads.discard(download)
            path=self.download_dir/filename
            good=download.state()==QWebEngineDownloadRequest.DownloadState.DownloadCompleted and path.is_file() and 0<path.stat().st_size<=524288
            if good:
                self.completed_downloads += 1
                item.setText(f'已保存 · {label}\n{path.stat().st_size / 1024:.1f} KB · TXT')
                item.setToolTip(str(path.resolve()))
            else:
                self.failed_downloads += 1
                item.setText(f'下载失败 · {label}\n文件未完成或大小异常')
            self.update_download_summary()
            if capture and self.capture is capture:
                if good:
                    self.finish(True,{'path':str(path.resolve()),'sa_id':self.command['sa_id'],
                                      'download_id':download.id(),'record_url':capture['record_url']})
                else:
                    self.finish(False,error='WOS 下载未完成或文件大小异常。')
            elif not capture:
                self.status.setText(f'手动下载已保存：{path}' if good else '手动下载未完成。')
        download.receivedBytesChanged.connect(progress)
        download.totalBytesChanged.connect(progress)
        download.isFinishedChanged.connect(finished)
        download.accept()

    def closeEvent(self,event):
        if self.command:
            self.finish(False,error='内置浏览器未连接，窗口已关闭。')
        for download in list(self.downloads):
            download.cancel()
        for popup in self.popups:
            popup.close()
        super().closeEvent(event)


def main():
    app=QApplication(sys.argv[:1])
    def emit(value):
        print(json.dumps(value,ensure_ascii=False),flush=True)
    window=BrowserWindow(Path(sys.argv[1]),emit,ORIGINS[0]+'/wos/')
    def read():
        for line in sys.stdin:
            try:
                window.incoming.put(json.loads(line))
            except ValueError:
                pass
        window.incoming.put({'action':'close'})
    threading.Thread(target=read,daemon=True).start()
    window.show()
    emit({'event':'ready'})
    app.exec()


if __name__=='__main__':
    main()
