# Decisions log

Append-only Y-statements for small, non-architectural decisions. Format:

> In the context of `<situation>`, facing `<concern>`, we decided for
> `<option>` and against `<alternatives>`, to achieve `<benefit>`, accepting
> `<downside>`.

Architectural decisions go to [`adr/`](adr/) instead.

---

- 2026-05-19 — In the context of bootstrapping the knowledge base, facing a
  Russian constitution but an English-only repo, we decided for a condensed
  English translation of the glossary (all terms preserved, prose tightened)
  and against a verbatim 1:1 translation or keeping Russian, to achieve a
  readable in-repo constitution, accepting that some authorial commentary is
  dropped.

- 2026-05-19 — In the context of mislabeled `feat(sN)` commits, facing a
  conflict with the canonical roadmap, we decided for keeping git history plus
  an audit doc and canonical numbering forward, and against rewriting history,
  to achieve honesty without destabilizing a published branch, accepting that
  old commit messages stay wrong (reconciled in `docs/audit/`).
