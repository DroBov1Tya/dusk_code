#!/usr/bin/env bash
# Decides which programs a release builds; prints `key=value` lines for $GITHUB_OUTPUT.
#   scripts/release-plan.sh <commit> <kind>      kind: stable | beta | nightly | manual
# A full release (stable, beta) or a manual run builds everything. A nightly builds a
# program only when files changed since the previous tag under its own crate, a crate it
# depends on, or the shared build inputs. No previous tag, or a diff that fails: build all.
set -euo pipefail

commit="$(git rev-parse --verify --quiet "${1:?commit}^{commit}")"
kind="${2:?kind}"

# Edit here when a crate gains or loses a dependency (clients -> server -> core -> protocol).
# The server embeds the web client (clients/browser), so both shipped clients depend on it.
shared=(Cargo.lock Cargo.toml .github/workflows/release.yml scripts/release-plan.sh resources
  scripts/build-web.sh projects/protocol projects/core projects/server projects/clients/browser)
cli_paths=("${shared[@]}" projects/clients/terminal)
desktop_paths=("${shared[@]}" projects/clients/desktop scripts/package-macos.sh scripts/package-linux.sh)

changed() { [[ -n "$(git diff --name-only "$previous" "$commit" -- "$@")" ]]; }

previous=""
cli=true
desktop=true
if [[ "$kind" == nightly ]]; then
  # Nearest tag behind the commit, so the tag being built is not its own predecessor.
  if previous="$(git describe --tags --abbrev=0 --match 'v[0-9]*' "$commit^" 2>/dev/null)"; then
    cli=false
    desktop=false
    if git diff --quiet --exit-code "$previous" "$commit" -- . >/dev/null 2>&1 || [[ $? -eq 1 ]]; then
      changed "${cli_paths[@]}" && cli=true
      changed "${desktop_paths[@]}" && desktop=true
    else
      cli=true desktop=true # the diff could not be computed
    fi
  else
    previous=""
  fi
fi

# The web client is embedded in both clients; its SBOM goes with either.
web=false
if [[ "$cli" == true || "$desktop" == true ]]; then web=true; fi

printf 'previous=%s\ncli=%s\ndesktop=%s\nweb=%s\n' "$previous" "$cli" "$desktop" "$web"
