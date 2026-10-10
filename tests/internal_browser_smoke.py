"""Offline native Chromium integration check; no real WOS network requests."""
import json
import sys
import tempfile
import time
from pathlib import Path

sys.path.insert(0,str(Path(__file__).resolve().parents[1]))
from internal_browser_worker import BrowserWindow, QApplication, QUrl, BASE
from PySide6.QtWebEngineCore import QWebEngineUrlRequestInterceptor


class Offline(QWebEngineUrlRequestInterceptor):
    def interceptRequest(self,info):
        if info.requestUrl().scheme() in ('http','https'):
            info.block(True)


app=QApplication([])
fixture=(BASE/'tests/fixtures/wos.html').read_text(encoding='utf-8')
temp=tempfile.TemporaryDirectory(prefix='wos-native-test-')
messages=[]
window=BrowserWindow(Path(temp.name),messages.append)
blocker=Offline(window)
window.profile.setUrlRequestInterceptor(blocker)
window.show()


def until(predicate,timeout=20):
    end=time.monotonic()+timeout
    while time.monotonic()<end:
        app.processEvents()
        if predicate():
            return
        time.sleep(.01)
    raise AssertionError('Native browser test timed out')


def html(url,callback=lambda:None):
    def done(ok):
        window.page.loadFinished.disconnect(done)
        assert ok
        callback()
    window.page.loadFinished.connect(done)
    window.page.setHtml(fixture,QUrl(url))


def js(source):
    answer=[]
    window.page.runJavaScript(source,answer.append)
    until(lambda:len(answer)>0)
    return answer[0]


try:
    ready=[]
    html('https://webofscience.clarivate.cn/wos/woscc/basic-search',lambda:ready.append(True))
    until(lambda:ready)
    assert js('location.origin')=='https://webofscience.clarivate.cn'
    # Page transitions are fixture-backed, while the actual renderer, script
    # callbacks, native download signal and disk writes are exercised unchanged.
    window.loaded=html
    command={'id':'native-search','action':'wos_search','title':'Synthetic paper',
             'doi':'10.1234/test','sa_id':'native-test','expires':int((time.time()+45)*1000)}
    window.incoming.put(command)
    until(lambda:messages,40)
    assert messages[-1]['ok'],messages[-1]
    record=messages[-1]['data']['record_url']
    assert record.endswith('WOS:000123456789012')
    js('window.wosOverlay=true')
    window.incoming.put({**command,'id':'native-export','action':'wos_export',
                         'expected_record_url':record,'expires':int((time.time()+45)*1000)})
    until(lambda:len(messages)==2,40)
    result=messages[-1]
    assert result['ok'],result
    path=Path(result['data']['path'])
    assert path.parent.resolve()==(Path(temp.name)/'downloads').resolve()
    assert 'TI\tAU\tAF\tSO\tPY\tC1\tUT\tDI' in path.read_text(encoding='utf-8')
    assert 'WOS:000123456789012' in path.read_text(encoding='utf-8')
    # Expired queue entries must not navigate, search or export again.
    before_url = window.page.url().toString()
    window.incoming.put({**command, 'id':'expired-search', 'expires':0})
    until(lambda:len(messages)==3)
    assert not messages[-1]['ok'] and '已过期' in messages[-1]['error']
    assert window.command is None
    assert window.page.url().toString() == before_url
    window.incoming.put(None)
    window.tick()
    assert window.completed_downloads == 1
    assert window.failed_downloads == 0
    assert window.download_list.count() == 1
    assert window.download_list.item(0).text().startswith('已保存 · Synthetic paper')
    assert '等待助手核验' in window.status.text()
    window.toggle_download_panel()
    assert window.download_panel.isHidden()
    assert window.panel_button.text() == '显示下载列表'
    window.toggle_download_panel()
    assert not window.download_panel.isHidden()
    window.resize(900, 620)
    app.processEvents()
    assert window.view.width() > 400
    assert window.download_panel.width() >= 240
    assert window.status.geometry().bottom() < window.centralWidget().height()
    window.resize(1200, 820)
    app.processEvents()
    window.grab().save(str(BASE/'runtime/internal-browser-preview.png'))
    print('PASS native Chromium: search -> detail -> Full Record -> TXT in private download directory')
    print('PASS no Chrome extension, no system Downloads, no external network')
    print('PASS download state, collapsible panel and compact window layout')
finally:
    window.close()
    # Chromium may hold its profile until process exit; preserve only test temp
    # files for OS cleanup if those handles have not been released yet.
    temp._finalizer.detach()
