"""Desktop-owned browser process; no extension or public command listener."""
from __future__ import annotations

import json
import os
import queue
import subprocess
import sys
import threading
import uuid
from pathlib import Path

from core import SafetyStop

BASE = Path(__file__).resolve().parent


class InternalBrowser:
    def __init__(self, root=None):
        self.root = Path(root or BASE / 'runtime' / 'browser')
        self.process = None
        self.ready = threading.Event()
        self.responses = queue.Queue()
        self.lock = threading.Lock()
        self.write_lock = threading.Lock()
        self.log = None

    @property
    def online(self):
        return self.ready.is_set() and self.process is not None and self.process.poll() is None

    def open(self):
        if self.process is not None and self.process.poll() is None:
            self._send({'action':'show'})
            return
        self.root.mkdir(parents=True, exist_ok=True)
        self.ready.clear()
        if self.log:
            self.log.close()
        self.log = (self.root / 'browser.log').open('a', encoding='utf-8')
        self.process = subprocess.Popen(
            [sys.executable, str(BASE / 'internal_browser_worker.py'), str(self.root)],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log,
            text=True, encoding='utf-8', bufsize=1,
            env={**os.environ,'PYTHONIOENCODING':'utf-8','PYTHONUNBUFFERED':'1'},
            creationflags=getattr(subprocess, 'CREATE_NO_WINDOW', 0))
        threading.Thread(target=self._read, args=(self.process,), daemon=True).start()

    def _read(self, process):
        try:
            for line in process.stdout:
                try:
                    message = json.loads(line)
                except ValueError:
                    continue
                if self.process is not process or not isinstance(message, dict):
                    continue
                if message.get('event') == 'ready':
                    self.ready.set()
                elif message.get('id'):
                    self.responses.put(message)
        finally:
            if self.process is process:
                self.ready.clear()

    def _send(self, message):
        try:
            with self.write_lock:
                self.process.stdin.write(json.dumps(message, ensure_ascii=False)+'\n')
                self.process.stdin.flush()
        except (AttributeError, OSError, ValueError) as exc:
            raise SafetyStop('内置浏览器未连接，请重新打开。') from exc

    def call(self, action, payload, timeout=90):
        import time
        if action not in ('wos_search', 'wos_export'):
            raise SafetyStop('内置浏览器仅用于 WOS 下载。')
        with self.lock:
            if not self.online:
                raise SafetyStop('内置浏览器未连接，请先打开并登录 WOS。')
            ident = uuid.uuid4().hex
            self._send({**payload, 'action':action, 'id':ident,
                        'expires':int((time.time()+timeout)*1000)})
            end = time.monotonic()+timeout
            while time.monotonic()<end:
                try:
                    result = self.responses.get(timeout=min(.5, max(.01, end-time.monotonic())))
                except queue.Empty:
                    if not self.online:
                        raise SafetyStop('内置浏览器未连接，下载窗口已关闭。')
                    continue
                if result.get('id') != ident:
                    continue
                if not result.get('ok'):
                    raise SafetyStop(result.get('error', '内置浏览器操作失败。'))
                return result.get('data', {})
            self._send({'action':'cancel','id':ident})
            raise SafetyStop('命令超时（网页已接收命令但未返回结果）；请检查内置浏览器。')

    def close(self):
        if self.process is not None and self.process.poll() is None:
            self._send({'action':'close'})
            try:
                self.process.wait(timeout=5)
            except subprocess.TimeoutExpired:
                self.process.terminate()
                self.process.wait(timeout=5)
        if self.log:
            self.log.close()
        self.ready.clear()
