# OTel計装つきの小さなエージェント作業。
# 作業: banto の crates/ の Rust 行数を数えて report.md を書く。
# 全ステップを span として記録し、trace.json に書き出す。
# このトレースが gate declare の証拠になる(自己申告ではなく、実際に何をしたかの観測)。
import json
import sys
from pathlib import Path

from opentelemetry import trace
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor, SpanExporter, SpanExportResult


class FileSpanExporter(SpanExporter):
    def __init__(self, path):
        self.path = Path(path)
        self.spans = []

    def export(self, spans):
        self.spans.extend(json.loads(s.to_json()) for s in spans)
        return SpanExportResult.SUCCESS

    def shutdown(self):
        self.path.write_text(json.dumps(self.spans, ensure_ascii=False, indent=1))


exporter = FileSpanExporter(Path(__file__).parent / "trace.json")
provider = TracerProvider()
provider.add_span_processor(SimpleSpanProcessor(exporter))
trace.set_tracer_provider(provider)
tracer = trace.get_tracer("agent-demo")

repo_root = Path(__file__).resolve().parents[2]
report_path = Path(__file__).parent / "report.md"

with tracer.start_as_current_span("agent.task") as task:
    task.set_attribute("task.description", "crates/ の Rust 行数を数えて report.md を書く")
    counts = {}
    with tracer.start_as_current_span("read_sources") as span:
        files = sorted((repo_root / "crates").rglob("*.rs"))
        for f in files:
            counts[str(f.relative_to(repo_root))] = len(f.read_text().splitlines())
        span.set_attribute("files.read", len(files))
        span.set_attribute("lines.total", sum(counts.values()))
    with tracer.start_as_current_span("write_report") as span:
        lines = ["# crates/ 行数レポート", ""]
        lines += [f"- {p}: {n}" for p, n in counts.items()]
        lines += ["", f"合計: {sum(counts.values())} 行({len(counts)} ファイル)"]
        report_path.write_text("\n".join(lines) + "\n")
        span.set_attribute("report.path", report_path.name)

provider.shutdown()
print(f"done: {len(counts)} files, report={report_path.name}, trace=trace.json")
