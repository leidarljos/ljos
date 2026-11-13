---
license: mit
language:
- en
pretty_name: ljos LongMemEval_S retrieval
size_categories:
- n<1K
---

# ljos LongMemEval_S retrieval

This dataset is one BM25+ session-retrieval run of the ljos scorer on cleaned LongMemEval_S.
The scorer is `longmemeval_s_recall` in the ljos tree.
It calls packset-core's BM25+ on the CPU.
It loads no encoder, downloads no model, and asks no remote judge.
Hit@5 is the share of scored questions with at least one evidence session among the first five ranks.
Recall@5 is the mean, over those questions, of the share of evidence sessions found in the top five.
Abstentions, whose ids end in `_abs`, are left out.

The questions are not in this dataset.
They stay in [`xiaowu0162/longmemeval-cleaned`](https://huggingface.co/datasets/xiaowu0162/longmemeval-cleaned) at revision `98d7416c24c778c2fee6e6f3006e7a073259d48f`, file `longmemeval_s_cleaned.json`.
That dataset's card says `license: mit`, so a copy is allowed.
This dataset does not copy the file.
The run names that revision and stops unless the sha256 is `d6f21ea9d60a0d56f34a05b609c79c88a451d2ae03597821ea3d5a9678c3a442`.

## Files

`out/summary.json` records the BM25+ figures for this run.
`hit@5` and `recall@5` are the six-decimal ratios the scorer prints.
`questions` is the number of scored questions.
`seconds` is the scorer's wall time, three digits past the point, from after the file is on disk through the scores.
`ljos` is the ljos version in the checkout.
`ljos_commit` is that checkout's git commit.
`packset` is the packset-core version the scorer links.
`dataset_repo` and `dataset_revision` name the upstream file.
`cpu_model` is the `model name` line from `/proc/cpuinfo`, or `unknown` when that file has none.
`date` is the day the run finished, in Coordinated Universal Time.

`out/per_question.csv` is one row per scored question, BM25+ only.
`question_id` and `question_type` identify the row.
`evidence` is how many labelled sessions the question has.
`found@1`, `found@5` and `found@10` count how many of those sessions sit inside that depth.
`hit@1`, `hit@5` and `hit@10` are 1 when the matching `found` column is above zero.
`recall@1`, `recall@5` and `recall@10` are `found` divided by `evidence`, six digits past the point.
The summary `hit@5` equals the fraction of rows with `hit@5` of 1.
The summary's `recall@5` is the mean of the unrounded per-question shares, then printed at six digits.
Averaging the already-printed `recall@5` column can differ in the last digit.

`run.sh` is the command that fetches the pinned file and runs the scorer.
`README.md` is this card.

## Reproduce

From a checkout of <https://github.com/leidarljos/ljos>:

```
sh bench/hf/run.sh
```

The command needs the Rust toolchain named in `rust-toolchain.toml`, `curl`, and `sha256sum` or `shasum`.
It writes `bench/hf/out/summary.json` and `bench/hf/out/per_question.csv`.

## Licence

The Massachusetts Institute of Technology licence.

Copyright © 2026, leiðarljós Developers.

The ljos sources are at <https://github.com/leidarljos/ljos>.
