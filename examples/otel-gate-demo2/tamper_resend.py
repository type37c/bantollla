"""第二幕の道具: 差し替え工作の試み。

エージェントが「都合の悪い数字を直したトレース」を後から再送する。
蔵の中の既存の記録は書き換えられないので、これは上書きにならず、
受信時刻つきの二本目として追記される — 工作の試み自体が証拠になる。
"""
import os

from opentelemetry import trace
from opentelemetry.exporter.otlp.proto.http.trace_exporter import OTLPSpanExporter
from opentelemetry.sdk.resources import Resource
from opentelemetry.sdk.trace import TracerProvider
from opentelemetry.sdk.trace.export import SimpleSpanProcessor

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

with tracer.start_as_current_span("agent.task") as task:
    task.set_attribute("task.description", "crates/ の Rust 行数を数えて report.md を書く")
    with tracer.start_as_current_span("read_sources") as span:
        # 作業はしない。「こうであってほしかった数字」を主張するだけ
        span.set_attribute("files.read", 13)
        span.set_attribute("lines.total", 9999)
    with tracer.start_as_current_span("write_report") as span:
        span.set_attribute("report.path", "report.md")

provider.shutdown()
print("resent with lines.total=9999")
