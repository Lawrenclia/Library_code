"""Automatic claim queue tests use synthetic records and a mocked browser."""
import copy
import unittest
from dataclasses import replace
from types import SimpleNamespace
from unittest.mock import Mock, patch

from approval import COMBINED_MISSING_IDS_CLAIM
from claim_batch import automatic_claim_selection, run_claim_batch
from core import Record, SafetyStop


def record(sa_id="demo-001", **changes):
    base = Record(2, "谭勋策", sa_id, "Synthetic", "", "", "00001", 1, "item-1",
                  "待处理", "作者不一致", "2")
    return replace(base, **changes)


def found(rec):
    row = {"saLzkId": rec.sa_id, "itemId": "item-1", "matchCount": 1,
           "markStatus": "待处理", "reason": "作者不一致", "gh": "00001", "remark": ""}
    comparison = [{"label": "认领状态", "sa": "测试员(00001)①", "library": "未认领"},
                  {"label": "作者信息", "sa": "测试员", "library": "Tester"}]
    return {"row": row, "comparison": comparison}


def prepared(search):
    data = {"staff_id": "00001", "sa_text": "测试员(00001)①", "item_id": "item-1",
            "person": {"id": "scholar-1", "wno": "00001", "name": "测试员",
                       "names": ["测试员", "Tester"]},
            "authors": [{"index": 0, "order": 1, "fullname": "Tester", "scholarId": "",
                         "eligible": True, "relations": []}]}
    return {"row": copy.deepcopy(search["row"]), "comparison": copy.deepcopy(search["comparison"]),
            "prepared": data, "suggested_index": 0}


class FakeRoster:
    def __init__(self, records):
        self.records = records
        self.assert_unchanged = Mock()


class ClaimBatchTests(unittest.TestCase):
    def test_selection_requires_one_exact_suggested_signature(self):
        rec = record()
        search = found(rec)
        result = prepared(search)
        selected = automatic_claim_selection(rec, search["row"], search["comparison"], result)
        self.assertEqual(selected[2]["fullname"], "Tester")
        for change in (None, True, 9):
            bad = copy.deepcopy(result)
            bad["suggested_index"] = change
            with self.assertRaises(SafetyStop):
                automatic_claim_selection(rec, search["row"], search["comparison"], bad)
        bad = copy.deepcopy(result)
        bad["prepared"]["person"]["names"] = ["Another"]
        with self.assertRaises(SafetyStop):
            automatic_claim_selection(rec, search["row"], search["comparison"], bad)

    def test_skips_red_candidate_then_completes_next_without_manual_approval(self):
        skipped = record("demo-skip", reason="通讯作者不一致")
        target = record("demo-ok")
        roster = FakeRoster([skipped, target])
        search = found(target)
        search["row"]["saLzkId"] = target.sa_id
        prep = prepared(search)
        proof = {"row": search["row"], "verified": True, "claimed": True, "staff_id": "00001",
                 "scholar_id": "scholar-1", "author": "Tester", "order": 1}
        bridge = Mock()
        bridge.call.side_effect = [search, prep, proof]
        completed_roster = FakeRoster([replace(skipped), replace(target, done=True)])
        closure = SimpleNamespace(completion=SimpleNamespace(roster=completed_roster))
        audit = Mock()
        with patch("claim_batch.auto_complete_claim", return_value=closure) as close:
            result = run_claim_batch(roster, [skipped, target], bridge, audit=audit)
        self.assertEqual(result.completed_ids, ("demo-ok",))
        self.assertIn("demo-skip", result.skipped)
        self.assertFalse(result.halted)
        self.assertEqual([call.args[0] for call in bridge.call.call_args_list],
                         ["search", "prepare_claim", "submit_claim"])
        self.assertTrue(bridge.call.call_args_list[-1].args[1]["confirmed"])
        close.assert_called_once()
        self.assertIn(("自动认领列表条目", "已跳过", "demo-skip"),
                      [call.args for call in audit.call_args_list])

    def test_ambiguous_author_closes_owned_drawer_and_continues(self):
        first, second = record("demo-a"), record("demo-b")
        roster = FakeRoster([first, second])
        search_a, search_b = found(first), found(second)
        ambiguous = prepared(search_a)
        ambiguous["suggested_index"] = None
        cleanup = copy.deepcopy(search_a)
        prep_b = prepared(search_b)
        proof = {"row": search_b["row"], "verified": True, "claimed": True, "staff_id": "00001",
                 "scholar_id": "scholar-1", "author": "Tester", "order": 1}
        bridge = Mock()
        bridge.call.side_effect = [search_a, ambiguous, cleanup, search_b, prep_b, proof]
        closed_roster = FakeRoster([first, replace(second, done=True)])
        with patch("claim_batch.auto_complete_claim",
                   return_value=SimpleNamespace(completion=SimpleNamespace(roster=closed_roster))):
            result = run_claim_batch(roster, [first, second], bridge)
        self.assertIn(first.sa_id, result.skipped)
        self.assertEqual(result.completed_ids, (second.sa_id,))
        self.assertEqual([call.args[0] for call in bridge.call.call_args_list],
                         ["search", "prepare_claim", "search", "search", "prepare_claim", "submit_claim"])

    def test_missing_doi_and_wos_unclaimed_uses_combined_note(self):
        target = record("demo-combined", reason="DOI和WOSID都不一致")
        roster = FakeRoster([target])
        search = found(target)
        search["row"]["reason"] = target.reason
        search["comparison"].extend([
            {"label": "DOI", "sa": "", "library": "10.1000/example"},
            {"label": "WOS记录号", "sa": "", "library": "WOS:000000000000001"},
        ])
        prep = prepared(search)
        proof = {"row": search["row"], "verified": True, "claimed": True, "staff_id": "00001",
                 "scholar_id": "scholar-1", "author": "Tester", "order": 1}
        bridge = Mock()
        bridge.call.side_effect = [search, prep, proof]
        closed_roster = FakeRoster([replace(target, done=True)])
        closure = SimpleNamespace(completion=SimpleNamespace(roster=closed_roster))
        with patch("claim_batch.auto_complete_claim", return_value=closure) as close:
            result = run_claim_batch(roster, [target], bridge)
        self.assertEqual(result.completed_ids, (target.sa_id,))
        self.assertEqual(close.call_args.kwargs["note"], COMBINED_MISSING_IDS_CLAIM)

    def test_combined_case_requires_both_sa_identifiers_blank(self):
        target = record("demo-incomplete", reason="DOI和WOSID都不一致")
        roster = FakeRoster([target])
        search = found(target)
        search["row"]["reason"] = target.reason
        search["comparison"].extend([
            {"label": "DOI", "sa": "10.1000/present", "library": "10.1000/example"},
            {"label": "WOS记录号", "sa": "", "library": "WOS:000000000000001"},
        ])
        bridge = Mock()
        bridge.call.return_value = search
        result = run_claim_batch(roster, [target], bridge)
        self.assertIn(target.sa_id, result.skipped)
        self.assertIn("并非同时为空", result.skipped[target.sa_id])
        bridge.call.assert_called_once_with("search", {"sa_id": target.sa_id})

    def test_uncertain_submit_marks_red_and_halts_without_next_record(self):
        first, second = record("demo-a"), record("demo-b")
        roster = FakeRoster([first, second])
        search = found(first)
        bridge = Mock()
        bridge.call.side_effect = [search, prepared(search), SafetyStop("timeout after submit")]
        result = run_claim_batch(roster, [first, second], bridge)
        self.assertTrue(result.halted)
        self.assertIn(first.sa_id, result.skipped)
        self.assertEqual(result.checked, 1)
        self.assertEqual(result.completed_ids, ())
        self.assertEqual([call.args[0] for call in bridge.call.call_args_list],
                         ["search", "prepare_claim", "submit_claim"])

    def test_cancel_and_limit_are_enforced(self):
        records = [record(f"demo-{index:03}") for index in range(101)]
        roster = FakeRoster(records)
        result = run_claim_batch(roster, records, Mock(), cancel=lambda: True)
        self.assertTrue(result.cancelled)
        self.assertEqual(result.checked, 0)
        with self.assertRaises(SafetyStop):
            run_claim_batch(roster, records, Mock(), limit=101)


if __name__ == "__main__":
    unittest.main()
