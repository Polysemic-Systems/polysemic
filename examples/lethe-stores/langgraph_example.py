#!/usr/bin/env python3
"""Run Lethe as a no-LLM LangGraph memory-lifecycle workflow."""

from __future__ import annotations

import uuid
from typing import Literal, TypedDict

from langgraph.graph import END, START, StateGraph

from lethe_contract import ErasureCoordinator, ErasureReport
from lethe_poc import Compose, PostgresStore, RedisStore, StoreCommandError


class LifecycleState(TypedDict, total=False):
    subject: str
    request_id: str
    remembered: int
    report: ErasureReport
    idempotent: bool
    verified: bool
    outcome: str


def build_graph(postgres: PostgresStore, redis: RedisStore):
    coordinator = ErasureCoordinator([postgres, redis], request_ledger=postgres)

    def remember(state: LifecycleState) -> dict[str, int]:
        subject = state["subject"]
        policy = "langgraph-preference-30d-v1"
        postgres.remember(
            subject,
            "prefers concise answers",
            policy,
            2_592_000,
            [0.2, 0.7, 0.1],
        )
        redis.remember(subject, "prefers concise answers", policy, 2_592_000)
        return {"remembered": 2}

    def erase(state: LifecycleState) -> dict[str, ErasureReport]:
        report = coordinator.erase_subject(state["subject"], state["request_id"])
        return {"report": report}

    def route_after_erasure(state: LifecycleState) -> Literal["replay", "escalate"]:
        return "replay" if state["report"].complete else "escalate"

    def replay(state: LifecycleState) -> dict[str, bool]:
        replayed = coordinator.erase_subject(state["subject"], state["request_id"])
        return {"idempotent": replayed == state["report"]}

    def verify(state: LifecycleState) -> dict[str, bool | str]:
        absent = postgres.verify_subject_absent(state["subject"])
        absent = absent and redis.verify_subject_absent(state["subject"])
        verified = state["report"].complete and state["idempotent"] and absent
        return {
            "verified": verified,
            "outcome": "erasure_complete" if verified else "human_review",
        }

    def escalate(_state: LifecycleState) -> dict[str, bool | str]:
        return {"verified": False, "outcome": "human_review"}

    builder = StateGraph(LifecycleState)
    builder.add_node("remember", remember)
    builder.add_node("erase", erase)
    builder.add_node("replay", replay)
    builder.add_node("verify", verify)
    builder.add_node("escalate", escalate)
    builder.add_edge(START, "remember")
    builder.add_edge("remember", "erase")
    builder.add_conditional_edges("erase", route_after_erasure)
    builder.add_edge("replay", "verify")
    builder.add_edge("verify", END)
    builder.add_edge("escalate", END)
    return builder.compile()


def run() -> int:
    compose = Compose()
    print("Starting stores for the LangGraph lifecycle example …", flush=True)
    compose.control("up", "--detach", "--wait", "--wait-timeout", "120")
    postgres = PostgresStore(compose)
    redis = RedisStore(compose)
    postgres.setup()
    postgres.reset_poc_data()
    redis.reset_poc_data()

    graph = build_graph(postgres, redis)
    result = graph.invoke(
        {
            "subject": "user:langgraph-demo",
            "request_id": f"langgraph-{uuid.uuid4()}",
        }
    )
    report = result["report"]
    print(f"remembered: {result['remembered']} store records")
    for store_result in report.stores:
        print(f"  {store_result}")
    print(f"aggregate: {report.status.value} — {report.receipt}")
    print(f"idempotent replay: {'yes' if result['idempotent'] else 'NO'}")
    print(f"graph outcome: {result['outcome']}")
    return 0 if result["verified"] else 1


if __name__ == "__main__":
    try:
        raise SystemExit(run())
    except StoreCommandError as error:
        print(f"error: {error}")
        raise SystemExit(1) from error
