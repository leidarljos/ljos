#!/usr/bin/env bash
#SBATCH --job-name=ljos-consent
#SBATCH --partition=cpu
#SBATCH --ntasks=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=12G
#SBATCH --time=00:40:00
set -euo pipefail
source_root=${1:?source directory}
cd "$source_root"
export CARGO_BUILD_JOBS="${SLURM_CPUS_PER_TASK:-4}"
if [[ -n "${2:-}" ]]; then
    mkdir -p build
    sha256sum "$2" > build/approval-baseline.sha256
    if python3 scripts/test_approval.py "$2" \
        ForwardedConsentWireTest.test_accept_records_consent \
        > build/approval-baseline.log 2>&1; then
        printf 'The baseline unexpectedly passed the replacement regression\n' >&2
        exit 1
    fi
    cat build/approval-baseline.log
fi
cargo build --locked --release -p ljos --bin ljos --bin ljos-mcp
cargo test --locked -p ljos --lib approval::tests
cargo test --locked -p ljos --bin ljos-mcp
cargo test --locked -p ljos --test mcp_forward
python3 scripts/test_approval.py target/release/ljos-mcp
sha256sum target/release/ljos target/release/ljos-mcp
