"""Batch WOS metadata export.

This only downloads: it drives the user's own bound WOS tab through the existing
``wos_search``/``wos_export`` extension commands and copies the captured Full Record
TXT into the submission intake folder. It never uploads, imports or pushes, because
those write to the production library and stay single-record with human confirmation.
"""
from __future__ import annotations

import re
from pathlib import Path

from automation import ImportStore, MAX_TXT, doi, wos, norm, parse_wos
from core import SafetyStop

# A bound tab that has been switched away or logged out fails every record, so stop
# instead of walking the whole roster with a dead bridge.
MAX_CONSECUTIVE_FAILURES = 3

# Outcomes that are about this one paper rather than about the session being broken.
# "WOS has no record" and "the result set is not a single record" are the expected,
# useful answers for a roster whose titles are often wrong -- treating them as
# failures would stop a list-driven run after the first three misses.
PER_RECORD_OUTCOMES = (
    '未找到记录',
    '不是可确认的唯一记录',
    '与名单冲突',
    '没有明确上海交通大学署名',
    '禁止导入',
)


def per_record_outcome(message):
    """True when the message describes this paper, not a broken session."""
    text = str(message)
    return any(marker in text for marker in PER_RECORD_OUTCOMES)


def wos_targets(roster, classification, papers):
    """Zero-match, unfinished records whose saved classification recommends WOS."""
    channel = {}
    for paper in papers:
        saved = classification.get(paper['id'])
        route = saved.get('import_route') if isinstance(saved, dict) else None
        value = route.get('recommended_channel') if isinstance(route, dict) else None
        for number in paper['rows']:
            channel[number] = value
    return [record for record in roster.records
            if record.matches == 0 and not record.done and channel.get(record.row) == 'WOS']


def roster_rows(roster):
    """Every zero-match, unfinished roster row, before duplicate papers are merged."""
    return [record for record in roster.records if record.matches == 0 and not record.done]


def all_targets(roster):
    """One record per paper, across every zero-match, unfinished paper.

    Two things are deliberate here:

    * The roster decides the scope, not the classification. The SA title is often the
      wrong one and WOS is exactly where the correct record is looked up, so limiting
      the run to records a model happened to label WOS would skip the ones that need it.
    * Rows are grouped by the same title+DOI key the intake uses, so a paper listed
      several times (each row carrying its own SA ID) produces a single TXT. The first
      row of the group represents the paper and supplies the SA ID in the filename.
    """
    seen, targets = set(), []
    for record in roster_rows(roster):
        key = (record.title, record.doi)
        if key in seen:
            continue
        seen.add(key)
        targets.append(record)
    return targets


def safe_name(record):
    """``<paper title>+<sa_lzk table ID>.txt``, sanitised and length-capped.

    The title is what a human reads when deciding which file to import, so it leads;
    the SA ID keeps two papers that share a title apart. Windows forbids \\ / : * ? " < >
    | in a name, and the whole path has to stay well inside MAX_PATH, so the title is
    truncated rather than the identifier.
    """
    title = re.sub(r'[\\/:*?"<>|\x00-\x1f]', ' ', str(record.title or ''))
    title = re.sub(r'\s+', ' ', title).strip(' .')
    ident = re.sub(r'[\\/:*?"<>|\x00-\x1f]', '_', str(record.sa_id or '')).strip() or 'record'
    room = 180 - len(ident) - len('+.txt')
    if room < 1:
        return f'{ident}.txt'
    title = title[:room].strip(' .')
    return f'{title}+{ident}.txt' if title else f'{ident}.txt'


class WOSDownload:
    """Search and download only. No SA status, affiliation or import transitions."""
    def __init__(self,bridge,store,unchanged,stop,audit):
        self.bridge,self.store,self.unchanged,self.stop,self.audit=bridge,store,unchanged,stop,audit

    def prepare(self,record):
        self.unchanged()
        cached=self.store.get(record)
        if cached:
            return cached
        query={'sa_id':record.sa_id,'title':record.title,'doi':doi(record.doi),'wos':wos(record.wos)}
        result=None
        for action in ('wos_search','wos_export'):
            if self.stop is not None and self.stop.is_set():
                raise SafetyStop('已暂停下载。')
            self.unchanged()
            result=self.bridge.call(action,query,timeout=75)
            self.audit(action,'已执行',record.sa_id)
        path=Path(result.get('path',''))
        if result.get('sa_id')!=record.sa_id or not path.is_absolute() or path.suffix.lower()!='.txt' or path.is_symlink():
            raise SafetyStop('无法确定本次导出的 TXT 文件。')
        if not path.is_file() or not 1<=path.stat().st_size<=MAX_TXT:
            raise SafetyStop('下载未完成或文件大小异常。')
        raw=path.read_bytes()
        candidate=parse_wos(raw)
        for name in ('doi','wos'):
            if query[name] and query[name]!=candidate[name]:
                raise SafetyStop(f'下载记录的 {name.upper()} 与名单冲突，未采纳。')
        confirmed=bool((query['doi'] or query['wos']) and norm(record.title)==norm(candidate['title']))
        self.store.archive(raw)
        state={'phase':'downloaded','candidate':candidate,'identity_confirmed':confirmed}
        self.store.save(record,state)
        return state


def export(targets, bridge, store, inbox,
           stop=None, progress=lambda text: None, unchanged=lambda: None,
           audit=lambda action, result, sa_id: None):
    """Export each target's Full Record through the user's own logged-in WOS tab.

    Takes the target list directly so the caller decides the scope; the WOS query field
    (WOS ID, else DOI, else title) is chosen by the extension per record.
    """
    targets = list(targets)
    inbox = Path(inbox)
    inbox.mkdir(parents=True, exist_ok=True)
    flow = WOSDownload(bridge, store, unchanged, stop, audit)
    exported, failed, unconfirmed = [], {}, []
    streak = 0
    for record in targets:
        if stop is not None and stop.is_set():
            progress(f'WOS 导出已暂停：已成功 {len(exported)} 条。')
            break
        progress(f'WOS 导出：已处理 {len(exported)}/{len(targets)}；原表第 {record.row} 行')
        try:
            # prepare() is idempotent: an already archived export is reused, not re-downloaded.
            state = flow.prepare(record)
            raw = store.bytes(state)
        except SafetyStop as exc:
            message = str(exc)
            failed[record.sa_id] = {'row': record.row, 'error': message,
                                    'per_record': per_record_outcome(message)}
            if failed[record.sa_id]['per_record']:
                # The page answered cleanly about this paper, so the session is healthy.
                streak = 0
                progress(f'第 {record.row} 行未导出：{message}')
                continue
            streak += 1
            progress(f'第 {record.row} 行导出暂停：{message}')
            if streak >= MAX_CONSECUTIVE_FAILURES:
                raise SafetyStop(
                    f'连续 {streak} 条 WOS 导出失败，已停止以免继续操作网页；'
                    f'已成功 {len(exported)} 条。请检查扩展里的 WOS 标签页绑定、登录状态和当前页面。')
            continue
        streak = 0
        sha = state['candidate']['sha256']
        if not state.get('identity_confirmed'):
            # The single-record flow asks a human here. The intake folder is adopted
            # automatically, so an unverified record must never be dropped into it.
            # The download stays in the archive for the automation page to confirm.
            unconfirmed.append({'sa_id': record.sa_id, 'row': record.row, 'title': record.title,
                                'doi': record.doi, 'archive': str(Path(store.root) / (sha + '.txt'))})
            progress(f'第 {record.row} 行已导出但身份未获强匹配，未放入待收目录，'
                     f'请核对存档文件后再用于提交准备。')
            continue
        target = inbox / safe_name(record)
        target.write_bytes(raw)
        exported.append({'sa_id': record.sa_id, 'row': record.row, 'title': record.title,
                         'doi': record.doi, 'file': str(target), 'sha256': sha})
    per_record = sum(1 for value in failed.values() if value['per_record'])
    return {'total': len(targets), 'exported': exported, 'failed': failed,
            'unconfirmed': unconfirmed, 'inbox': str(inbox),
            'not_exported': per_record, 'session_failures': len(failed) - per_record,
            'stopped': bool(stop is not None and stop.is_set())}


def plan(document, classification_dir=None):
    """(classification, papers) for a roster document.

    Takes the papers document produced by ``read_papers``. Three roster shapes float
    around this codebase (a papers document, a ``core.Roster``, a zero-match queue) and
    only the document carries the fingerprint under the same key shape the saved
    results were written against, so the roster object is deliberately not accepted.
    """
    from submission_prepare import CLASSIFICATION_DIR, saved_classification
    classification = saved_classification(document['sha256'], classification_dir or CLASSIFICATION_DIR)
    if not classification:
        raise SafetyStop('没有与当前名单指纹匹配的分类结果，请先运行“开始 / 继续分类”。')
    return classification, document['papers']


def default_store():
    from paper_classify import BASE
    return ImportStore(BASE / 'runtime' / 'wos-downloads')


def default_inbox():
    from paper_classify import BASE
    from submission_prepare import INBOX_NAME
    return BASE / 'runtime' / 'submission' / INBOX_NAME
