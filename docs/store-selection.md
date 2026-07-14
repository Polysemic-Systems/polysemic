# Lethe store selection — first evidence pass

Research date: 2026-07-14.

Documentation is a useful proxy for ecosystem demand, but not a substitute for
watching teams operate deletion across production replicas, exports, and backups.
This pass selects the first POC adapters; interviews should challenge the choice.

## Primary-source signals

| Ecosystem | Store signals relevant to agent memory |
|---|---|
| LangGraph | Its persistence documentation recommends `PostgresStore`, `MongoDBStore`, or `RedisStore` for production. Its production memory example uses PostgreSQL. |
| Mem0 | Supports PGVector and Redis among many vector stores; Qdrant is its default when no vector-store configuration is supplied. |
| LlamaIndex | Lists PostgreSQL, Redis, and Qdrant with deletion support in its vector-store matrix. |
| AutoGen | Documents persistent ChromaDB memory and a Redis vector-memory implementation. |

Sources:

- [LangGraph persistence](https://docs.langchain.com/oss/python/langgraph/persistence)
- [LangGraph memory](https://docs.langchain.com/oss/python/langgraph/add-memory)
- [Mem0 supported vector databases](https://docs.mem0.ai/components/vectordbs/overview)
- [LlamaIndex vector stores](https://developers.llamaindex.ai/python/framework/module_guides/storing/vector_stores/)
- [AutoGen memory and RAG](https://microsoft.github.io/autogen/dev/user-guide/agentchat-user-guide/memory.html)

## Decision

Build **PostgreSQL+pgvector** and **Redis** first.

PostgreSQL is the clearest durable system of record across the surveyed stacks;
pgvector lets the same adapter cover a common vector-memory deployment without
introducing another database. Redis appears in three of the four surveyed
ecosystems and exercises different lifecycle semantics: native TTL, key indexes,
and asynchronous `UNLINK` deletion.

Qdrant is the next candidate because Mem0 uses it by default and both Mem0 and
LlamaIndex support it. It should move ahead of Redis or PostgreSQL if design
partners repeatedly identify it as their authoritative memory store.

## Questions web research cannot answer

1. Which store is authoritative when the same memory is copied into several?
2. Must erasure cover replicas, AOF/WAL, snapshots, analytics exports, and backups?
3. What proof will security or compliance reviewers accept as a receipt?
4. How long can an erasure workflow remain partially complete?
5. Do teams need subject deletion, namespace deletion, predicate deletion, or all three?
6. Who owns retention policy changes and exceptions?

These are the first design-partner interview questions. They determine the real
control-plane product more than another adapter does.
