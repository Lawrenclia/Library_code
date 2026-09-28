import tempfile
import tkinter as tk
import unittest
from pathlib import Path
from unittest.mock import patch
from app import App
from core import Journal, file_hash, read_roster
from operation_log import OperationLog
from tests.test_roster_write import make_roster


class UnifiedTests(unittest.TestCase):
    def setUp(self):
        self.tmp=tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.path=Path(self.tmp.name)/'list.xlsx'
        make_roster(self.path)
        self.root=tk.Tk()
        self.root.withdraw()
        self.reader=patch('classify_app.read_papers',return_value={'papers':[{'rows':[2]}]})
        self.reader.start()
        self.addCleanup(self.reader.stop)
        self.app=App(self.root,Journal(Path(self.tmp.name)/'unified.db'),auto_load=False,unified=True,
                     operation_log=OperationLog(Path(self.tmp.name)/'preview-log.txt'))
        self.app.loaded(read_roster(self.path))

    def tearDown(self):
        self.app.closing=False
        self.app.classifier.busy=False
        self.app.set_busy(False)
        self.root.update_idletasks()
        self.app.close()

    def test_all_features_in_one_notebook_and_shared_busy_state(self):
        self.assertEqual([self.app.tabs.tab(t,'text') for t in self.app.tabs.tabs()],['人工处理','自动化 / 认领','批量分类 / 核对','零匹配提交准备','设置'])
        self.assertEqual([self.app.analysis_tabs.tab(t,'text') for t in self.app.analysis_tabs.tabs()],['批量分类 / 导入渠道','单条核对'])
        self.assertIs(self.app.classifier.root,self.root)
        self.app.classifier.set_busy(True)
        self.assertTrue(self.app.busy)
        self.assertEqual(str(self.app.complete_button['state']),'disabled')
        self.app.classifier.set_busy(False)
        self.assertFalse(self.app.busy)
        self.app.set_busy(True)
        self.assertEqual(str(self.app.classifier.start_button['state']),'disabled')
        self.assertTrue(self.app.classifier.external_busy)

    def test_classified_record_navigates_without_web_or_write(self):
        record=self.app.roster.records[0]
        before=file_hash(self.path)
        self.app.review_classified({'rows':[record.row],'title':record.title,'doi':record.doi})
        self.assertEqual(self.app.current.sa_id,record.sa_id)
        self.assertEqual(str(self.app.tabs.select()),str(self.app.manual_page))
        self.assertIsNone(self.app.bridge)
        self.assertEqual(file_hash(self.path),before)

    def test_close_waits_for_batch(self):
        self.app.classifier.set_busy(True)
        self.app.close()
        self.assertTrue(self.app.closing)
        self.assertTrue(self.app.classifier.stop.is_set())
        self.assertTrue(self.root.winfo_exists())

    def test_first_batch_initializes_archive_and_next_batch_reuses_it(self):
        self.app.automation_panel.runtime=Path(self.tmp.name)/'wos-imports'
        self.assertIsNone(self.app.automation_panel.store)
        record=self.app.roster.records[0]
        stores=[]

        def download(targets,bridge,store,inbox,**kwargs):
            # Exercise the same first archive access as WOSFlow.prepare.
            self.assertIsNone(store.get(record))
            self.assertTrue(store.path.is_file())
            stores.append(store)

        def run(job,callback,status,**kwargs):
            job()

        with patch.object(self.app,'run',side_effect=run), \
             patch('wos_batch.export',side_effect=download), \
             patch('wos_batch.default_store',side_effect=lambda: __import__('automation').ImportStore(Path(self.tmp.name)/'downloads')), \
             patch('wos_batch.default_inbox',return_value=Path(self.tmp.name)/'inbox'):
            self.app._start_wos_export([record],'测试下载')
            self.app._start_wos_export([record],'测试下载')
        self.assertEqual(len(stores),2)
        self.assertEqual(stores[0].path,stores[1].path)
        self.assertIsNone(self.app.automation_panel.store)

    def test_integrated_layout_controls_within_window(self):
        self.root.deiconify()
        self.root.geometry('960x740')
        self.root.update()
        for tab in self.app.tabs.tabs():
            self.app.tabs.select(tab)
            self.root.update()
            for widget in (self.app.complete_button,self.app.status_label,self.app.model_panel.copy_button,
                           self.app.classifier.start_button,self.app.classifier.channel_button,self.app.classifier.review_button):
                if widget.winfo_viewable():
                    self.assertGreater(widget.winfo_height(),10)
                    self.assertLessEqual(widget.winfo_rootx()+widget.winfo_width(),self.root.winfo_rootx()+self.root.winfo_width())
                    self.assertLessEqual(widget.winfo_rooty()+widget.winfo_height(),self.root.winfo_rooty()+self.root.winfo_height())

    def test_every_page_keeps_its_controls_on_screen(self):
        # Regression: the WOS download button existed but Tk silently unmapped the whole
        # footer row on a short window. The older check above only inspected widgets that
        # were already viewable, so it skipped exactly the broken case and stayed green.
        self.root.deiconify()
        interactive=('TButton','Button','TCombobox','TEntry','Checkbutton','TCheckbutton')

        def walk(node):
            for child in node.winfo_children():
                yield child
                yield from walk(child)

        def hidden_by_notebook(widget):
            # A Notebook's unselected pane is hidden on purpose, not clipped.
            node=widget
            while node is not None and node is not self.root:
                parent=node.nametowidget(node.winfo_parent())
                if parent.winfo_class()=='TNotebook' and not node.winfo_ismapped():
                    return True
                node=parent
            return False

        for size in ('1180x900','960x740'):
            self.root.geometry(size)
            for _ in range(3):
                self.root.update()
            height=self.root.winfo_height()
            width=self.root.winfo_width()
            for tab in self.app.tabs.tabs():
                self.app.tabs.select(tab)
                for _ in range(3):
                    self.root.update()
                page=self.app.tabs.nametowidget(tab)
                if not page.winfo_ismapped():
                    continue
                for widget in walk(page):
                    if widget.winfo_class() not in interactive or hidden_by_notebook(widget):
                        continue
                    label=widget.cget('text') if 'text' in widget.keys() else widget.winfo_class()
                    where=f'{size} / {self.app.tabs.tab(tab,"text")} / {label}'
                    self.assertTrue(widget.winfo_ismapped(),
                                    f'{where} 没有被布局，控件存在但看不见')
                    top=widget.winfo_rooty()-self.root.winfo_rooty()
                    right=widget.winfo_rootx()-self.root.winfo_rootx()+widget.winfo_width()
                    self.assertGreaterEqual(top,0,f'{where} 顶部越界')
                    self.assertLessEqual(top+widget.winfo_height(),height,f'{where} 底部越界')
                    self.assertLessEqual(right,width,f'{where} 右侧越界')
