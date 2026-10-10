import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest.mock import Mock, patch

from core import SafetyStop
from internal_browser import InternalBrowser


class BrowserBridgeTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.browser=InternalBrowser(Path(self.temp.name))
        self.browser.process=Mock()
        self.browser.process.poll.return_value=None
        self.browser.ready.set()

    def test_only_wos_download_actions_are_allowed(self):
        with self.assertRaises(SafetyStop):
            self.browser.call('import_submit',{})
        self.browser.process.stdin.write.assert_not_called()

    def test_disconnected_browser_never_dispatches(self):
        self.browser.process.poll.return_value=1
        with self.assertRaisesRegex(SafetyStop,'未连接'):
            self.browser.call('wos_search',{})

    def test_private_pipe_matches_command_and_preserves_unicode(self):
        def send(message):
            self.browser.responses.put({'id':'old','ok':True,'data':{'path':'wrong'}})
            self.browser.responses.put({'id':message['id'],'ok':True,'data':{'path':'中文目录/论文.txt'}})
        with patch.object(self.browser,'_send',side_effect=send):
            result=self.browser.call('wos_export',{'sa_id':'test'},timeout=1)
        self.assertEqual(result['path'],'中文目录/论文.txt')

    def test_chinese_error_is_reported_to_download_queue(self):
        def send(message):
            self.browser.responses.put({'id':message['id'],'ok':False,'error':'WOS 未找到记录'})
        with patch.object(self.browser,'_send',side_effect=send):
            with self.assertRaisesRegex(SafetyStop,'未找到记录'):
                self.browser.call('wos_search',{},timeout=1)

    def test_closed_native_window_does_not_fall_back_to_chrome(self):
        from app import App
        app=Mock(internal_browser=self.browser,bridge=Mock())
        self.browser.ready.clear()
        self.assertIs(App.wos_browser(app),self.browser)

    def test_shutdown_closes_only_owned_process(self):
        self.browser.close()
        sent=json.loads(self.browser.process.stdin.write.call_args.args[0])
        self.assertEqual(sent['action'],'close')
        self.browser.process.wait.assert_called_once_with(timeout=5)
        self.assertFalse(self.browser.ready.is_set())

    def test_old_process_cannot_mark_reopened_browser_ready(self):
        old = Mock(stdout=io.StringIO('{"event":"ready"}\n{"id":"old","ok":true}\n'))
        self.browser.ready.clear()
        self.browser._read(old)
        self.assertFalse(self.browser.ready.is_set())
        self.assertTrue(self.browser.responses.empty())

    def test_non_object_messages_do_not_break_reader(self):
        self.browser.process.stdout = io.StringIO('null\n[]\n42\n{"id":"valid","ok":true}\n')
        self.browser._read(self.browser.process)
        self.assertEqual(self.browser.responses.get_nowait()['id'], 'valid')
