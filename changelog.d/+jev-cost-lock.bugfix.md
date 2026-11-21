Lock `jev-cost.toml` updates across concurrent `record_cost_in` and `count_in` writers and replace the file atomically so concurrent replies never drop calls or spend.
