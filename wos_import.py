"""Scoped local-export import queue; reuses the existing write-ahead WOS flow.

Planning is local-only. Execution needs a reviewed plan and always rechecks SA.
No completion/claim/platform writes: only remote-already-processed rows sync to 1.
"""
from dataclasses import dataclass, field
from pathlib import Path

from automation import MAX_TXT, WOSFlow, classify, doi, identity, norm, parse_wos, wos
from core import SafetyStop
from pilot import OWNER
from roster_write import reconcile_processed


@dataclass(frozen=True)
class ImportItem:
    record: object
    status: str
    message: str
    path: str = ""
    sha256: str = ""
    candidate: dict = field(default_factory=dict)


@dataclass(frozen=True)
class ImportPlan:
    sha256: str
    owner: str
    items: tuple
    file_errors: tuple = ()


@dataclass
class ImportResult:
    roster: object
    outcomes: list = field(default_factory=list)
    halted: bool = False
    cancelled: bool = False


def require_owner(owner):
    if not str(owner).strip():
        raise SafetyStop("请选择负责人。")
    if owner != OWNER:
        raise SafetyStop("当前自动导入仅开放谭勋策负责的任务，不操作其他负责人的记录。")


def build_plan(roster, owner, limit, inbox, store):
    require_owner(owner)
    if type(limit) is not int or not 1 <= limit <= 100:
        raise SafetyStop("本轮条数须为 1–100 的整数。")
    roster.assert_unchanged()
    records = [r for r in roster.records if r.owner == owner and not r.done and not r.skipped and r.matches == 0][:limit]
    files, errors = {}, []
    # Never recursively traverse Downloads or collect unrelated files.
    for path in sorted(Path(inbox).glob("*.txt")):
        try:
            if path.is_symlink() or not path.is_file() or not 1 <= path.stat().st_size <= MAX_TXT:
                raise SafetyStop("文件大小/类型不支持")
            candidate = parse_wos(path.read_bytes())
            files.setdefault(candidate["sha256"], (path, candidate))
        except (SafetyStop, OSError) as exc:
            errors.append(f"{path.name}：{exc}")
    items, seen = [], set()
    for record in records:
        try:
            state = store.get(record)
            if state:
                candidate = parse_wos(store.bytes(state))
                identity(record, candidate)
                if not state.get("identity_confirmed"):
                    raise SafetyStop("存档文献身份待人工核验，请使用单条导入。")
                phase = state.get("phase")
                if phase == "upload_intent":
                    raise SafetyStop("上次上传结果不明，请核对上传窗口；不会再次上传。")
                if phase not in ("exported", "import_intent", "imported", "push_intent", "pushed"):
                    raise SafetyStop("导入日志状态未知，请人工核验。")
                if phase == "pushed":
                    items.append(ImportItem(record, "pushed", "此前已核验推送；仍需关联平台号并处理 SA 任务。", candidate=candidate))
                    continue
                path = ""
                label = "ready" if phase == "exported" else "resume"
            else:
                # Prefer exact supplied identifiers; title similarity is never sufficient.
                matches = []
                for path, candidate in files.values():
                    if ((wos(record.wos) and wos(record.wos) == candidate["wos"]) or
                        (doi(record.doi) and doi(record.doi) == candidate["doi"]) or
                        norm(record.title) == norm(candidate["title"])):
                        matches.append((path, candidate))
                if not matches:
                    raise SafetyStop("没有对应的单篇 WOS TXT；先下载元数据或使用单条导入。")
                # Distinct byte exports with the same UT may carry different metadata.
                if len(matches) != 1:
                    raise SafetyStop("存在多个不同的候选文件，请人工选定单条 TXT。")
                path, candidate = matches[0]
                if not identity(record, candidate):
                    raise SafetyStop("缺少精确编号或题名发生变化，需在单条流程确认身份。")
                label = "ready"
            if store.other_writes(record, candidate):
                raise SafetyStop("同一论文已有其他名单的导入记录，请关联已有条目，禁止重复导入。")
            paper_key = candidate["wos"]
            if paper_key in seen:
                raise SafetyStop("与本轮前面的论文重复，只导入一篇，其余名单待关联。")
            seen.add(paper_key)
            items.append(ImportItem(record, label, "文件身份匹配；导入前仍需核验本库缺失。" if label == "ready" else
                                    "继续核验既有批次，不重发已提交操作。", str(path), candidate["sha256"], candidate))
        except (SafetyStop, OSError) as exc:
            items.append(ImportItem(record, "deferred", str(exc)))
    roster.assert_unchanged()
    return ImportPlan(roster.sha256, owner, tuple(items), tuple(errors))


def run_import_plan(plan, roster, bridge, store, *, reviewed=False, stop=lambda: False,
                    progress=lambda text: None, audit=lambda action, result, sa_id: None):
    require_owner(plan.owner)
    if reviewed is not True:
        raise SafetyStop("请先核验本轮文献身份及本库缺失，并确认导入与推送设置。")
    roster.assert_unchanged()
    if plan.sha256 != roster.sha256:
        raise SafetyStop("预检后名单改变，请重新预检。")
    current = {r.sa_id: r for r in roster.records}
    ids = [item.record.sa_id for item in plan.items]
    if len(ids) != len(set(ids)) or len(ids) > 100 or any(
        item.record != current.get(item.record.sa_id) or item.record.owner != OWNER or item.record.done or
        item.record.skipped or item.record.matches != 0 for item in plan.items
    ):
        raise SafetyStop("导入计划包含范围外、重复或已变化的记录，请重新预检。")
    result = ImportResult(roster)

    def unchanged():
        result.roster.assert_unchanged()
        if stop():
            raise SafetyStop("已暂停后续步骤；已发出的提交请先核验结果。")

    def record_outcome(item, status, message):
        result.outcomes.append({"sa_id": item.record.sa_id, "title": item.record.title,
                                "status": status, "message": message})
        audit("WOS 导入队列", "已暂停" if status == "halted" else "已跳过" if status == "deferred" else "已执行",
              item.record.sa_id)

    flow = WOSFlow(bridge, store, unchanged, progress, audit)
    for index, item in enumerate(plan.items, 1):
        if stop():
            result.cancelled = True
            break
        record = item.record
        progress(f"WOS 导入 {index}/{len(plan.items)} · {record.sa_id}")
        # Even deferred files are checked for already-processed SA tasks first.
        try:
            unchanged()
            fresh = bridge.call("search", {"sa_id": record.sa_id})
            audit("WOS 导入前核验 SA", "已执行", record.sa_id)
            completion = reconcile_processed(result.roster, record, fresh.get("row"))
            if completion:
                result.roster = completion.roster
                record_outcome(item, "synced", "后台已处理，Excel 已同步为 1；未执行导入。")
                continue
        except Exception as exc:
            record_outcome(item, "halted", str(exc) if isinstance(exc, SafetyStop) else "连接或名单读写异常，请检查后继续。")
            result.halted = True
            break
        if item.status in ("deferred", "pushed"):
            record_outcome(item, item.status, item.message)
            continue
        try:
            if classify(record, fresh).route != "wos":
                raise SafetyStop("后台不再是未处理零匹配任务；未执行导入。")
            if item.path:
                flow.prepare_file(record, item.path, item.sha256)
            state = store.get(record)
            if not state or state["candidate"]["sha256"] != item.sha256 or not state.get("identity_confirmed"):
                raise SafetyStop("导入存档与预检计划不一致，请重新核验。")
            identity(record, parse_wos(store.bytes(state)))
            if state["phase"] == "exported" and store.other_writes(record, state["candidate"]):
                raise SafetyStop("同一论文已由其他名单提交，不重复导入。")
        except (SafetyStop, OSError) as exc:
            record_outcome(item, "deferred", str(exc))
            continue
        try:
            state = flow.proceed(record)
            if state["phase"] != "pushed":
                raise SafetyStop("导入尚未核验到推送完成，请检查既有批次。")
            record_outcome(item, "pushed", f"批次 {(state.get('batch') or {}).get('batchNumber', '')} 已核验推送；待平台号关联及 SA 结案。")
        except Exception as exc:
            # Any failure inside the production flow stops the queue. Never replay writes.
            record_outcome(item, "halted", str(exc) if isinstance(exc, SafetyStop) else "导入结果不明，请核对批次，勿重复提交。")
            result.halted = True
            break
    result.cancelled = stop()
    return result
