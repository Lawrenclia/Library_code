"""Explicit local TXT -> backend import UI, independent from the download queue."""
import threading
import tkinter as tk
from pathlib import Path
from tkinter import ttk, filedialog

from automation import ImportStore
from core import SafetyStop
from notices import messages as messagebox
from ui_theme import P, style_text
from wos_import import build_plan, require_owner, run_import_plan

LABELS = {"ready": "可导入", "resume": "待续验", "deferred": "待核验", "pushed": "已推送·待关联",
          "synced": "原已处理", "halted": "已暂停"}


class WOSImportPanel:
    def __init__(self, app, page):
        self.app = app
        self.plan = None
        self.running = False
        self.stop = threading.Event()
        self.entries = {}
        self.file_errors = ()
        self.limit = tk.StringVar(value="5")
        self.inbox = tk.StringVar(value=str(app.automation_panel.runtime.parent / "submission" / "待收导出"))
        self.reviewed = tk.BooleanVar(value=False)
        self.status = tk.StringVar(value="先预检已下载的单篇 TXT，再确认本库缺失并导入。")
        ttk.Label(page, text="WOS 导入", style="Title.TLabel").pack(anchor="w", pady=(2, 8))
        source = ttk.Frame(page)
        source.pack(fill="x", pady=(0, 8))
        ttk.Label(source, text="TXT 目录").pack(side="left")
        self.folder_entry = ttk.Entry(source, textvariable=self.inbox, width=10)
        self.folder_entry.pack(side="left", fill="x", expand=True, padx=6)
        app.button(source, "选择", self.choose_folder).pack(side="right")
        row = ttk.Frame(page)
        row.pack(fill="x", pady=(0, 8))
        ttk.Label(row, text="本轮").pack(side="left")
        self.limit_box = ttk.Spinbox(row, from_=1, to=100, textvariable=self.limit, width=5)
        self.limit_box.pack(side="left", padx=6)
        ttk.Label(row, text="条 · 负责人：").pack(side="left")
        ttk.Label(row, textvariable=app.owner).pack(side="left")
        actions = ttk.Frame(page)
        actions.pack(fill="x", pady=(0, 8))
        app.button(actions, "预检文件", self.preview).pack(side="left")
        self.start_button = app.button(actions, "开始 / 继续导入", self.start, style="Primary.TButton")
        self.start_button.pack(side="left", padx=5)
        app.button(actions, "单条处理", self.open_single).pack(side="left")
        self.stop_button = ttk.Button(actions, text="暂停", command=self.cancel, state="disabled")
        self.stop_button.pack(side="right")
        self.intro_label = ttk.Label(page, text="需连接 SA 比对页，并绑定“数据导入与批次管理”。已有 TXT 时无需 WOS 页。\n只导入当前负责人范围；新导入不自动认领、填平台号或批准完成。",
                  wraplength=430, style="Muted.TLabel")
        self.intro_label.pack(anchor="w", pady=(0, 8))
        # Reserve the consent/status footer before allocating the flexible list area.
        self.status_label = ttk.Label(page, textvariable=self.status, wraplength=850)
        self.status_label.pack(side="bottom", fill="x", pady=(8, 0))
        self.consent = ttk.Checkbutton(page, variable=self.reviewed,
                                      text="已核验本轮可导入项的文献身份及本库缺失")
        self.consent.pack(side="bottom", anchor="w", pady=(6, 0))
        panes = ttk.Panedwindow(page, orient="vertical")
        panes.pack(fill="both", expand=True)
        listing = ttk.Frame(panes)
        panes.add(listing, weight=2)
        self.tree = ttk.Treeview(listing, columns=("title", "state"), show="headings", height=5, selectmode="browse")
        self.tree.heading("title", text="论文 / 名单")
        self.tree.heading("state", text="导入状态")
        self.tree.column("title", width=280, minwidth=140)
        self.tree.column("state", width=140, minwidth=125, stretch=False)
        for tag, color in (("ready", P.amber), ("resume", P.amber), ("deferred", P.red),
                           ("halted", P.red), ("pushed", P.green), ("synced", P.green)):
            self.tree.tag_configure(tag, foreground=color)
        scrollbar = ttk.Scrollbar(listing, command=self.tree.yview)
        self.tree.configure(yscrollcommand=scrollbar.set)
        scrollbar.pack(side="right", fill="y")
        self.tree.pack(fill="both", expand=True)
        detail = ttk.Frame(panes)
        panes.add(detail, weight=1)
        self.details = tk.Text(detail, height=4, width=20, wrap="word", state="disabled")
        style_text(self.details)
        bar = ttk.Scrollbar(detail, command=self.details.yview)
        self.details.configure(yscrollcommand=bar.set)
        bar.pack(side="right", fill="y")
        self.details.pack(fill="both", expand=True)
        self.tree.bind("<<TreeviewSelect>>", self.show_selected)
        for variable in (self.limit, self.inbox, app.owner):
            variable.trace_add("write", lambda *_: self.invalidate(clear=True))
        def fit(event):
            for label in (self.intro_label, self.status_label):
                label.configure(wraplength=max(250, event.width - 30))
        page.bind("<Configure>", fit)

    def store(self):
        panel = self.app.automation_panel
        if panel.store is None:
            panel.store = ImportStore(panel.runtime)
        return panel.store

    def invalidate(self, clear=False):
        self.plan = None
        self.reviewed.set(False)
        if clear:
            self.entries = {}
            self.file_errors = ()
            self.render()
            self.status.set("范围或目录已改变，请重新预检。")

    def choose_folder(self):
        if self.app.busy:
            return
        selected = filedialog.askdirectory(parent=self.app.root)
        if selected:
            self.inbox.set(selected)

    def set_busy(self, busy):
        if not busy:
            self.running = False
        for widget in (self.folder_entry, self.limit_box, self.consent):
            widget.configure(state="disabled" if busy else "normal")
        self.stop_button.configure(state="normal" if self.running and busy and not self.stop.is_set() else "disabled")

    def preview(self):
        if self.app.busy:
            return
        try:
            require_owner(self.app.owner.get())
            if not self.app.roster:
                raise SafetyStop("请先读取 list.xlsx。")
            text = self.limit.get().strip()
            if not text.isdigit() or not 1 <= int(text) <= 100:
                raise SafetyStop("本轮条数须为 1–100 的整数。")
            inbox = Path(self.inbox.get())
            if not inbox.is_dir():
                raise SafetyStop("TXT 目录不存在，请先下载元数据或选择已有导出目录。")
            roster, owner, limit = self.app.roster, self.app.owner.get(), int(text)
            self.invalidate()
        except SafetyStop as exc:
            messagebox.showwarning("暂未预检", str(exc), parent=self.app.root)
            return
        def ready(plan):
            self.plan = plan
            self.file_errors = plan.file_errors
            self.entries = {i.record.sa_id: {"sa_id": i.record.sa_id, "title": i.record.title,
                "status": i.status, "message": i.message, "candidate": i.candidate, "path": i.path} for i in plan.items}
            self.render()
            counts = {s: sum(i.status == s for i in plan.items) for s in LABELS}
            self.status.set(f"预检 {len(plan.items)} 条 · 可导入 {counts['ready']} · 待续验 {counts['resume']} · 待核验 {counts['deferred']}"
                            + (f" · {len(plan.file_errors)} 个文件格式不适用。" if plan.file_errors else ""))
            if not plan.items:
                self.status.set("当前负责人没有未完成、未跳过的零匹配记录。")
        self.app.run(lambda: build_plan(roster, owner, limit, inbox, self.store()), ready,
                     "正在预检本地 WOS TXT（不操作网页、不调用模型）…", log_action="预检 WOS 导入文件")

    def start(self):
        app = self.app
        if app.busy:
            return
        try:
            require_owner(app.owner.get())
            if not self.plan or not self.plan.items:
                raise SafetyStop("请先预检文件。")
            if self.plan.owner != app.owner.get() or self.plan.sha256 != app.roster.sha256:
                raise SafetyStop("负责人或名单已变化，请重新预检。")
            app.roster.assert_unchanged()
            if not app.bridge or not app.bridge.online:
                raise SafetyStop("请连接 SA 比对页，并在扩展绑定后台“数据导入与批次管理”页。")
            if not self.reviewed.get():
                raise SafetyStop("请逐条核验预检列表的文献身份及本库缺失，再勾选确认。")
            count = sum(i.status in ("ready", "resume") for i in self.plan.items)
            if not messagebox.askyesno("确认本轮 WOS 导入", f"负责人：{app.owner.get()}\n本轮 {count} 条可导入 / 待续验。\n\n"
                "每条先重查 SA；原已处理的只同步 Excel。\n所属机构：上海交通大学；说明：SA补充-名单ID。\n"
                "按 PPT 查重、优先级合并、新增并推送，可能合并已有文献元数据。\n"
                "不上传 PDF；新导入不会直接写完成标记。结果不明时停止，不重复提交。\n\n是否继续？", parent=app.root):
                return
            plan, roster = self.plan, app.roster
        except SafetyStop as exc:
            messagebox.showwarning("暂未导入", str(exc), parent=app.root)
            return
        self.stop.clear()
        self.running = True
        progress = app.automation_panel.progress.put
        def audit(action, outcome, sa_id):
            try:
                app.operation_log.record(action, outcome, sa_id)
            except Exception:
                progress("log.txt 保存失败；导入状态仍保存在独立日志中，请检查目录。")
        def done(result):
            self.running = False
            app.roster = result.roster
            app.clear_selection()
            app.populate()
            for outcome in result.outcomes:
                self.entries[outcome["sa_id"]].update(outcome)
            self.render()
            self.invalidate()
            self.status.set(("已暂停，请核验网页后重新预检。" if result.halted else "已暂停后续步骤。" if result.cancelled else "本轮结束。")
                            + "已推送仍需关联平台号及 SA 结案；新导入未写 Excel 完成标记。")
            app.status.set(self.status.get())
            self.set_busy(False)
        app.run(lambda: run_import_plan(plan, roster, app.bridge, self.store(), reviewed=True,
            stop=self.stop.is_set, progress=progress, audit=audit), done, "开始逐篇核验并导入 WOS…", log_action="WOS 导入队列")

    def cancel(self):
        self.stop.set()
        self.stop_button.configure(state="disabled")
        self.status.set("已请求暂停后续步骤；已发出的提交不能撤回，请等待回读。")

    def render(self):
        self.tree.delete(*self.tree.get_children())
        self.details.configure(state="normal")
        self.details.delete("1.0", "end")
        self.details.configure(state="disabled")
        for sa_id, entry in self.entries.items():
            self.tree.insert("", "end", iid=sa_id, values=(entry["title"], LABELS[entry["status"]]), tags=(entry["status"],))
        if self.entries:
            self.tree.selection_set(next(iter(self.entries)))
            self.show_selected()

    def show_selected(self, _event=None):
        selection = self.tree.selection()
        if not selection:
            return
        entry = self.entries[selection[0]]
        candidate = entry.get("candidate", {})
        text = f"{entry['sa_id']}\n{entry['title']}\n{entry['message']}"
        if candidate:
            text += (f"\n\nWOS：{candidate['wos']}\nDOI：{candidate['doi'] or '—'}\n"
                     f"作者：{candidate['authors']}\n{candidate['journal']} · {candidate['year']}\n"
                     f"单位：{candidate['affiliation']}")
        if entry.get("path"):
            text += "\n文件：" + entry["path"]
        if self.file_errors:
            text += "\n\n目录内未采纳的文件（最多显示 5 项）：\n" + "\n".join(self.file_errors[:5])
        self.details.configure(state="normal")
        self.details.delete("1.0", "end")
        self.details.insert("1.0", text)
        self.details.configure(state="disabled")

    def open_single(self):
        if self.app.busy:
            return
        selection = self.tree.selection()
        if selection and self.app.roster:
            record = next((r for r in self.app.roster.records if r.sa_id == selection[0]), None)
            if record:
                self.app.owner.set(record.owner)
                self.app.task_view.set("done" if record.done else "pending")
                self.app.select_owner()
                self.app.tree.selection_set(record.sa_id)
                self.app.select_record()
        self.app.tabs.select(self.app.automation_page)
        self.app.automation_panel.subtabs.select(self.app.automation_panel.wos_page)
