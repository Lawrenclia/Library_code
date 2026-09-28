"""Shared API settings. Secrets stay in the existing Windows DPAPI store."""
import json
import tkinter as tk
from pathlib import Path
from tkinter import ttk

from core import SafetyStop
from model_review import API_BASE, DEFAULT_MODEL, MODELS
from notices import messages as messagebox
from paper_classify import atomic_json

DEFAULTS = {"review": DEFAULT_MODEL, "classification": "deepseek-chat", "submission": "deepseek-chat"}


def read_preferences(path):
    path = Path(path)
    if not path.exists():
        return dict(DEFAULTS)
    try:
        data = json.loads(path.read_text(encoding="utf-8"))
        if not isinstance(data, dict) or set(data) != set(DEFAULTS) or any(v not in MODELS for v in data.values()):
            raise ValueError()
        return data
    except (OSError, ValueError, TypeError):
        raise SafetyStop("模型偏好配置无法读取，暂用默认值；可在设置页重新保存。") from None


class SettingsPanel:
    def __init__(self, app, page, runtime):
        self.app = app
        self.path = Path(runtime) / "model_settings.json"
        self.key = tk.StringVar()
        self.status = tk.StringVar()
        self.key_status = tk.StringVar()
        self.models = {name: tk.StringVar(value=value) for name, value in DEFAULTS.items()}
        self.controls = []
        ttk.Label(page, text="设置", style="Title.TLabel").pack(anchor="w", pady=(4, 14))
        ttk.Label(page, text="模型服务 · 所有功能共用本机 API 密钥", style="Muted.TLabel").pack(anchor="w")
        ttk.Label(page, text="API 地址").pack(anchor="w", pady=(16, 4))
        address = ttk.Entry(page)
        address.insert(0, API_BASE)
        address.configure(state="readonly")
        address.pack(fill="x")
        ttk.Label(page, text="保留交大固定地址与 HTTPS 校验；不向其他服务发送密钥。", style="Muted.TLabel",
                  wraplength=430).pack(anchor="w", pady=(4, 12))
        ttk.Label(page, textvariable=self.key_status).pack(anchor="w")
        self.entry = ttk.Entry(page, textvariable=self.key, show="●")
        self.entry.pack(fill="x", pady=(5, 8))
        self.controls.append(self.entry)
        actions = ttk.Frame(page)
        actions.pack(fill="x")
        app.button(actions, "保存密钥", self.save_key, style="Primary.TButton").pack(side="left")
        app.button(actions, "测试连接", self.test_connection).pack(side="left", padx=8)
        ttk.Label(page, text="密钥使用 Windows 账号加密保存，不回显、不写入 Git 或日志。\n测试连接只读取可用模型，不发送名单。",
                  wraplength=430, style="Muted.TLabel").pack(anchor="w", pady=(8, 14))
        for name, label in (("review", "单条核对模型"), ("classification", "批量分类模型"), ("submission", "材料准备模型")):
            row = ttk.Frame(page)
            row.pack(fill="x", pady=4)
            ttk.Label(row, text=label, width=16).pack(side="left")
            combo = ttk.Combobox(row, textvariable=self.models[name], values=MODELS, state="readonly", width=23)
            combo.pack(side="left", fill="x", expand=True)
            self.controls.append(combo)
        app.button(page, "保存模型偏好", self.save_models).pack(anchor="w", pady=(10, 8))
        ttk.Label(page, textvariable=self.status, wraplength=430).pack(anchor="w", pady=6)
        try:
            values = read_preferences(self.path)
            for name, value in values.items():
                self.models[name].set(value)
            self.apply_models(values)
        except SafetyStop as exc:
            self.status.set(str(exc))
        self.refresh()

    def refresh(self):
        configured = self.app.model_client.key_store.configured()
        self.key_status.set("密钥已配置 · 如需替换，请输入新密钥" if configured else "尚未配置 · 请输入校方 API Key")
        if self.app.classifier:
            self.app.classifier.key_status.set("密钥已配置" if configured else "请到设置配置密钥")

    def set_busy(self, busy):
        for control in self.controls:
            control.configure(state="disabled" if busy else "readonly" if isinstance(control, ttk.Combobox) else "normal")

    def save_key(self):
        if self.app.busy:
            return
        try:
            self.app.model_client.key_store.save(self.key.get().strip())
            self.key.set("")
            self.app.model_client.available = None
            self.app.model_panel.clear()
            self.status.set("密钥已加密保存，可测试连接。")
            self.refresh()
        except Exception:
            messagebox.showwarning("未保存", "密钥格式或加密保存失败，请检查后重试。", parent=self.app.root)

    def apply_models(self, values):
        self.app.model_panel.model.set(values["review"])
        self.app.model_panel.clear()
        if self.app.classifier:
            self.app.classifier.model.set(values["classification"])
            self.app.classifier.change_model()
        if self.app.submission_panel:
            self.app.submission_panel.model.set(values["submission"])

    def save_models(self):
        if self.app.busy:
            return
        values = {name: variable.get() for name, variable in self.models.items()}
        if any(value not in MODELS for value in values.values()):
            self.status.set("请选择支持的模型。")
            return
        try:
            self.path.parent.mkdir(parents=True, exist_ok=True)
            atomic_json(self.path, values)
            self.apply_models(values)
            self.status.set("模型偏好已保存并应用；各工作页仍可临时选择模型。")
        except (OSError, SafetyStop):
            self.status.set("模型偏好未保存，请检查目录权限。")

    def test_connection(self):
        if self.app.busy:
            return
        self.status.set("正在测试连接，不发送论文或名单…")
        def ready(models):
            self.status.set("连接成功 · " + "、".join(models) if models else "连接成功，但未返回支持的模型名。")
        self.app.run(self.app.model_client.models, ready, "测试模型服务连接…", log_action="测试模型连接")
