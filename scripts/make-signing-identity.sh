#!/bin/sh
# Makes the certificate releases of the desktop app are signed with, once. It is
# self-signed: it does not stand for a Developer ID and does not notarize, it only lets
# every release carry the same signature, so macOS keeps the permissions and keychain
# access given to an earlier version.
#   sh scripts/make-signing-identity.sh <out dir>
# Writes <out dir>/duskcode-signing.p12 and its password, and prints what to store as the
# repository secrets MACOS_SIGN_P12 (base64 of the .p12) and MACOS_SIGN_PASSWORD. Keep
# the .p12 out of the repository; anyone holding it can sign as duskcode.
set -eu

fail() { printf 'make-signing-identity: %s\n' "$1" >&2; exit 1; }

[ $# -eq 1 ] || fail "usage: make-signing-identity.sh <out dir>"
out="$1"
command -v openssl >/dev/null 2>&1 || fail "openssl is needed"
[ ! -e "$out/duskcode-signing.p12" ] || fail "$out/duskcode-signing.p12 exists; a new one would break the chain of releases"
mkdir -p "$out"
umask 077
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

cat > "$work/cert.cnf" <<CNF
[req]
distinguished_name = name
x509_extensions = ext
prompt = no
[name]
CN = Duskcode Release Signing
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
CNF
openssl req -x509 -newkey rsa:3072 -nodes -days 3650 -config "$work/cert.cnf" \
  -keyout "$work/key.pem" -out "$work/cert.pem" 2>/dev/null
password="$(openssl rand -base64 24)"
# The legacy algorithms are the ones the macOS keychain imports.
openssl pkcs12 -export -legacy -inkey "$work/key.pem" -in "$work/cert.pem" \
  -name "Duskcode Release Signing" -passout "pass:$password" \
  -out "$out/duskcode-signing.p12" 2>/dev/null \
  || openssl pkcs12 -export -inkey "$work/key.pem" -in "$work/cert.pem" \
    -name "Duskcode Release Signing" -passout "pass:$password" \
    -out "$out/duskcode-signing.p12"
printf '%s\n' "$password" > "$out/duskcode-signing.password"
printf 'Wrote %s/duskcode-signing.p12 and its password.\n' "$out"
printf 'Store them as repository secrets:\n'
printf '  base64 < %s/duskcode-signing.p12 | gh secret set MACOS_SIGN_P12\n' "$out"
printf '  gh secret set MACOS_SIGN_PASSWORD < %s/duskcode-signing.password\n' "$out"
