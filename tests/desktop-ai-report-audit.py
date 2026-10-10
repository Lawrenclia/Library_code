"""Independently compare actual HTTP input, SQLite and the native Excel report."""
import argparse
import base64
import hashlib
import json
import sqlite3
from pathlib import Path
import xml.etree.ElementTree as ET
from zipfile import ZipFile

MAIN = "{http://schemas.openxmlformats.org/spreadsheetml/2006/main}"
REL = "{http://schemas.openxmlformats.org/officeDocument/2006/relationships}"


def rows(path, name):
    with ZipFile(path) as archive:
        book = ET.fromstring(archive.read("xl/workbook.xml"))
        sheet = next(s for s in book.find(MAIN + "sheets") if s.attrib["name"] == name)
        refs = ET.fromstring(archive.read("xl/_rels/workbook.xml.rels"))
        target = next(r.attrib["Target"] for r in refs if r.attrib["Id"] == sheet.attrib[REL + "id"])
        target = target.lstrip("/") if target.startswith("/") else "xl/" + target
        shared = []
        if "xl/sharedStrings.xml" in archive.namelist():
            shared = ["".join(v.itertext()) for v in ET.fromstring(archive.read("xl/sharedStrings.xml"))]
        sheet_rows = ET.fromstring(archive.read(target)).find(MAIN + "sheetData")
        output = []
        for row in sheet_rows:
            cells = {}
            for cell in row:
                key = "".join(c for c in cell.attrib["r"] if c.isalpha())
                value = cell.find(MAIN + "v")
                text = value.text if value is not None else ""
                if cell.attrib.get("t") == "s":
                    text = shared[int(text)]
                elif cell.attrib.get("t") == "inlineStr":
                    text = "".join(cell.find(MAIN + "is").itertext())
                cells[key] = text
            output.append(cells)
        return output


def audit(acceptance):
    evidence = json.loads(acceptance.read_text(encoding="utf-8"))
    assert evidence["passed"] and evidence["synthetic"] and not evidence["real_provider_verified"]
    for run in evidence["runs"]:
        if run.get("terminated"):
            stopped = run["termination"]
            assert stopped["owned_process_id"] == run["process_id"] and stopped["exit_code"] == 0
            assert stopped["command"] == ["taskkill", "/PID", str(run["process_id"]), "/T", "/F"]
            original = base64.b64decode(stopped["output_base64"], validate=True)
            assert hashlib.sha256(original).hexdigest() == stopped["output_sha256"]
            assert f"PID {run['process_id']}".encode("ascii") in original
    audits = []
    for scenario in evidence["scenarios"]:
        root = Path(scenario["workspace"])
        with sqlite3.connect(root.joinpath("workspace.sqlite3").as_uri() + "?mode=ro", uri=True) as db:
            raw = db.execute("SELECT data FROM ai_queues ORDER BY rowid").fetchall()
            tasks = [json.loads(r[0]) for r in db.execute("SELECT data FROM tasks ORDER BY rowid")]
            writes = db.execute("SELECT count(*) FROM attempts").fetchone()[0]
        assert len(raw) == 1 and writes == 0
        queue = json.loads(raw[0][0])
        assert queue["status"] == "completed" and queue["cursor"] == 4
        assert len(queue["targets"]) == 4
        assert all(t["stage"] == "pending" and not t["record"]["done"] for t in tasks)
        parts = rows(root / "ai-native-report.xlsx", "AI 批量结果")[1:]
        assert len(parts) > 2, "The complete long input should require multiple Excel cells"
        assert [int(r["I"]) for r in parts] == list(range(1, len(parts) + 1))
        full = "".join(r["J"] for r in parts)
        digest = hashlib.sha256(full.encode("utf-8")).hexdigest()
        assert all(r["H"] == digest for r in parts)
        assert json.loads(full) == queue, "Excel must retain the entire SQLite queue"
        calls = scenario["requests"]
        assert [c["number"] for c in calls] == [1, 2, 3, 4]
        for call, outcome in zip(calls, queue["outcomes"]):
            assert outcome["attempt"]["input"] == call["input"]
            assert json.loads(call["body"]["messages"][1]["content"]) == call["input"]
            assert outcome["attempt"]["task_id"] == outcome["id"]
            task = next(t for t in tasks if t["id"] == outcome["id"])
            if outcome["status"] == "classified":
                result = task["classification"]
                assert result["queue_attempt_id"] == outcome["attempt"]["id"]
                records = [json.loads(e["text"]) for e in task["evidence"] if e["kind"] == "ai_classification"]
                assert any(r["result"] == result and r["sources"] == call["input"]["sources"] and not r["platform_verified"] for r in records)
        original = next(s for s in calls[0]["input"]["sources"] if s["id"] == "long-original-source")
        assert original["text"] == "Full original source 重要资料🧪。" * 1600
        assert queue["targets"][0]["record"]["staff_id"] == "000000000000001"
        if scenario["name"] != "ordinary":
            added = next(t for t in tasks if t["id"] == "ai-native-new")
            assert added["classification"] is None and all(t["id"] != added["id"] for t in queue["targets"])
        audits.append({"scenario": scenario["name"], "workspace": str(root), "report_parts": len(parts), "complete_queue_sha256": digest, "actual_http_inputs": len(calls), "business_stage": "pending", "platform_writes": writes})
    output = acceptance.with_name("ai-report-audit.json")
    output.write_text(json.dumps({"passed": True, "independent_xml_sqlite_audit": True, "scenarios": audits}, ensure_ascii=False, indent=2), encoding="utf-8")
    print(f"Native AI report audit passed: {len(audits)} scopes, 20 exact HTTP inputs, complete long Unicode input/SQLite/Excel, original text identifiers, unchanged business states. Evidence: {output}")


if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    parser.add_argument("acceptance", type=Path)
    audit(parser.parse_args().acceptance)
