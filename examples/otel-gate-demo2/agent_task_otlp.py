"""続編デモの作業体(正直版)。

一本目と同じ作業 — crates/ の Rust 行数を数えて report.md を書く。
違いは一つだけ: トレースを手元の trace.json に書くのではなく、
認証付き OTLP/HTTPS で独立 Collector(信頼境界の向こう)へ送る。

実行者は非特権ユーザー。送る資格(トークン)は持つが、
着地点(蔵)には手が届かない。
"""
import os
import sys
from pathlib import Path

from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor

repo_root = Path(sys.argv[1] if len(sys.argv) > 1 else ".")
report_path = Path(sys.argv[2] if len(sys.argv) > 2 else "report.md")

exporter = OTLPSpanExporter(
    endpoint=os.environ.get("OTLP_ENDPOINT", "https://127.0.0.1:4318/v1/traces"),
    certificate_file=os.environ["OTLP_CA_CERT"],
    headers={"Authorization": f"Bearer {os.environ['OTLP_TOKEN']}"},
)
provider = TracerProvider(
    resource=Resource.create({"service.name": "banto-demo-agent", "actor": "agent:claude"})
)
provider.add_span_processor(SimpleSpanProcessor(exporter))
trace.set_tracer_provider(provider)
tracer = trace.get_tracer("otel-gate-demo2")


def render(counts: dict) -> str:
    lines = ["# crates/ 行数レポート", ""]
    lines += [f"- {f}: {n}" for f, n in counts.items()]
    lines += ["", f"合計: {sum(counts.values())} 行({len(counts)} ファイル)"]
    return "\n".join(lines) + "\n"


with tracer.start_as_current_span("agent.task") as task:
    task.set_attribute("task.description", "crates/ の Rust 行数を数えて report.md を書く")
    with tracer.start_as_current_span("read_sources") as span:
        files = sorted((repo_root / "crates").rglob("*.rs"))
        counts = {str(f.relative_to(repo_root)): len(f.read_text().splitlines()) for f in files}
        span.set_attribute("files.read", len(files))
        span.set_attribute("lines.total", sum(counts.values()))
    with tracer.start_as_current_span("write_report") as span:
        report_path.write_text(render(counts))
        span.set_attribute("report.path", report_path.name)

provider.shutdown()
print(f"done: {len(counts)} files, {sum(counts.values())} lines -> {report_path}")
