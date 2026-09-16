"""OR-Tools CP-SAT adapter for the Constraint Lab optimization IR.

Reads `griff.constraint-lab-opt` v1 problem records (JSON lines), solves each
to proven optimality, and writes one solve record per problem (JSON lines, in
input order) in the shape `optir::SolveRecord` parses. The adapter is an
*untrusted* oracle: the Rust side re-scores every witness and accepts an
optimum only when the solver proved it (`optir::verify_record`).

Optional agreement pass (`--agreement`): lexicographically, at the proven
optimum, maximize how many of the record's reference `(var, value)`
pairs hold — the tie-insensitive ceiling of a model's agreement with a
reference (e.g. the human tab).

Escalation: every problem is first solved in a process pool with `--threads`
workers and `--time-limit`; a record without a proven optimum (or, with
`--agreement`, without a proven agreement pass) is re-solved sequentially with
`--retry-threads` and `--retry-limit`, and its solver identity says so. On the
first corpus run a single-worker search left ~2% of lines unproven after 120 s,
while the multi-worker portfolio proved the same lines in about a second.

Usage:
    python solve_opt.py IN.jsonl OUT.jsonl [--jobs N] [--threads T]
                        [--time-limit SECONDS] [--agreement]
                        [--retry-threads T] [--retry-limit SECONDS]
"""

import argparse
import json
import multiprocessing as mp
import sys
import time

import ortools
from ortools.sat.python import cp_model

SCHEMA = "griff.constraint-lab-opt"
SCHEMA_VERSION = 1

STATUS = {
    cp_model.OPTIMAL: "optimal",
    cp_model.FEASIBLE: "feasible",
    cp_model.INFEASIBLE: "infeasible",
    cp_model.MODEL_INVALID: "model_invalid",
    cp_model.UNKNOWN: "unknown",
}


def build(problem):
    """Encodes the IR as a CP-SAT model; returns (model, vars, objective)."""
    m = cp_model.CpModel()
    domains = [v["domain"] for v in problem["vars"]]
    xs = [
        m.NewIntVarFromDomain(cp_model.Domain.FromValues(d), v["name"])
        for v, d in zip(problem["vars"], domains)
    ]
    for h in problem["hard"]:
        if h["kind"] != "allowed":
            raise ValueError(f"unknown hard kind {h['kind']}")
        m.AddAllowedAssignments([xs[h["a"]], xs[h["b"]]], [tuple(t) for t in h["tuples"]])

    terms = []
    for i, t in enumerate(problem["objective"]):
        kind = t["kind"]
        if kind == "unary":
            table = {v: c for v, c in t["costs"]}
            dom = domains[t["var"]]
            rows = [(v, table.get(v, 0)) for v in dom]
            c = m.NewIntVarFromDomain(
                cp_model.Domain.FromValues(sorted({r[1] for r in rows})), f"u{i}"
            )
            m.AddAllowedAssignments([xs[t["var"]], c], rows)
            terms.append(c)
        elif kind == "pair":
            table = {(a, b): c for a, b, c in t["costs"]}
            rows = [
                (a, b, table.get((a, b), 0))
                for a in domains[t["a"]]
                for b in domains[t["b"]]
            ]
            c = m.NewIntVarFromDomain(
                cp_model.Domain.FromValues(sorted({r[2] for r in rows})), f"p{i}"
            )
            m.AddAllowedAssignments([xs[t["a"]], xs[t["b"]], c], rows)
            terms.append(c)
        elif kind == "abs_diff":
            da, db = domains[t["a"]], domains[t["b"]]
            span = max(abs(max(da) - min(db)), abs(max(db) - min(da)))
            d = m.NewIntVar(0, span, f"d{i}")
            m.AddAbsEquality(d, xs[t["a"]] - xs[t["b"]])
            terms.append(t["weight"] * d)
        elif kind == "not_equal":
            b = m.NewBoolVar(f"n{i}")
            m.Add(xs[t["a"]] != xs[t["b"]]).OnlyEnforceIf(b)
            m.Add(xs[t["a"]] == xs[t["b"]]).OnlyEnforceIf(b.Not())
            terms.append(t["weight"] * b)
        else:
            raise ValueError(f"unknown objective kind {kind}")
    objective = sum(terms) if terms else 0
    return m, xs, objective


def solver_for(threads, time_limit):
    s = cp_model.CpSolver()
    s.parameters.num_workers = threads
    s.parameters.max_time_in_seconds = time_limit
    return s


def solve_one(args):
    line, threads, time_limit, agreement, tag = args
    rec = json.loads(line)
    if rec.get("schema") != SCHEMA or rec.get("version") != SCHEMA_VERSION:
        raise ValueError(f"unsupported record schema {rec.get('schema')}/{rec.get('version')}")
    problem = rec["problem"]
    started = time.perf_counter()
    m, xs, objective = build(problem)
    m.Minimize(objective)
    s = solver_for(threads, time_limit)
    status = s.Solve(m)
    out = {
        "id": rec["id"],
        "fingerprint_hex": rec["fingerprint_hex"],
        "solver": {
            "name": "ortools/cp-sat",
            "version": f"{ortools.__version__} (num_workers={threads}, time_limit={time_limit}s{tag})",
        },
        "status": STATUS.get(status, "unknown"),
        "objective": None,
        "bound": None,
        "witness": None,
        "wall_us": 0,
        "agreement": None,
    }
    if status in (cp_model.OPTIMAL, cp_model.FEASIBLE):
        out["objective"] = int(round(s.ObjectiveValue()))
        out["bound"] = int(round(s.BestObjectiveBound()))
        out["witness"] = [int(s.Value(x)) for x in xs]
    out["wall_us"] = int((time.perf_counter() - started) * 1e6)

    if agreement and status == cp_model.OPTIMAL and rec.get("reference"):
        # Lexicographic in one solve: with scale = len(reference) + 1, the
        # minimum of scale*cost - matches has the minimum cost first and the
        # most matches among cost-optimal assignments second. (Pinning
        # `objective == optimum` as a constraint is far slower in CP-SAT.)
        m2, xs2, objective2 = build(problem)
        matches = []
        for var, value in rec["reference"]:
            b = m2.NewBoolVar(f"ref{var}")
            m2.Add(xs2[var] == value).OnlyEnforceIf(b)
            matches.append(b)
        for x, v in zip(xs2, out["witness"]):
            m2.AddHint(x, v)
        scale = len(matches) + 1
        m2.Minimize(scale * objective2 - sum(matches))
        s2 = solver_for(threads, time_limit)
        status2 = s2.Solve(m2)
        ag = {"status": STATUS.get(status2, "unknown"), "matched": None, "witness": None}
        if status2 in (cp_model.OPTIMAL, cp_model.FEASIBLE):
            ag["witness"] = [int(s2.Value(x)) for x in xs2]
            ag["matched"] = int(sum(s2.Value(b) for b in matches))
        out["agreement"] = ag
    return json.dumps(out, separators=(",", ":"))


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("input")
    ap.add_argument("output")
    ap.add_argument("--jobs", type=int, default=mp.cpu_count())
    ap.add_argument("--threads", type=int, default=1)
    ap.add_argument("--time-limit", type=float, default=120.0)
    ap.add_argument("--agreement", action="store_true")
    ap.add_argument("--retry-threads", type=int, default=mp.cpu_count())
    ap.add_argument("--retry-limit", type=float, default=300.0)
    a = ap.parse_args()

    with open(a.input, encoding="utf-8") as f:
        lines = [l for l in f if l.strip()]
    work = [(l, a.threads, a.time_limit, a.agreement, "") for l in lines]
    started = time.perf_counter()
    results = []
    with mp.Pool(a.jobs) as pool:
        for i, result in enumerate(pool.imap(solve_one, work, chunksize=1), 1):
            results.append(result)
            if i % 500 == 0 or i == len(work):
                print(f"{i}/{len(work)} solved, {time.perf_counter() - started:.1f}s", file=sys.stderr)

    def unproven(result):
        r = json.loads(result)
        if r["status"] != "optimal":
            return True
        return a.agreement and r["agreement"] is not None and r["agreement"]["status"] != "optimal"

    retry = [i for i, r in enumerate(results) if unproven(r)]
    print(f"escalating {len(retry)} unproven records", file=sys.stderr)
    for n, i in enumerate(retry, 1):
        results[i] = solve_one((lines[i], a.retry_threads, a.retry_limit, a.agreement, ", escalated"))
        print(f"  escalated {n}/{len(retry)}, {time.perf_counter() - started:.1f}s", file=sys.stderr)

    with open(a.output, "w", encoding="utf-8", newline="\n") as out:
        for result in results:
            out.write(result + "\n")


if __name__ == "__main__":
    main()
