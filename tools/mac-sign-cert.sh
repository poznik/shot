#!/bin/sh
# Makes the self-signed certificate that signs Shot.app: tools/mac-sign-cert.sh DIR [--upload]
#
# Once, not per release: macOS keeps the Screen Recording permission for the builds signed with the
# same certificate (see tools/bundle-mac.sh), so a new certificate costs every user the permission
# once more. Writes DIR/shot-sign.p12 and DIR/shot-sign.password; keep both, outside the
# repository. With --upload it also stores them as the GitHub secrets MAC_SIGN_P12 and
# MAC_SIGN_PASSWORD, which CI signs with. OPENSSL picks the openssl to run.
set -eu

usage() {
  echo "usage: $0 DIR [--upload]" >&2
  exit 2
}
dir=
upload=no
for arg in "$@"; do
  case "$arg" in
    --upload) upload=yes ;;
    -*) usage ;;
    *) [ -z "$dir" ] || usage; dir="$arg" ;;
  esac
done
[ -n "$dir" ] || usage
openssl=${OPENSSL:-openssl}
p12="$dir/shot-sign.p12"
password="$dir/shot-sign.password"
if [ -e "$p12" ]; then
  echo "$p12 is there already: a new certificate would cost users the permission" >&2
  exit 1
fi
mkdir -p "$dir"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

cat > "$tmp/cert.cnf" <<EOF
[req]
distinguished_name = dn
x509_extensions = ext
prompt = no
[dn]
CN = Shot self-signed
[ext]
basicConstraints = critical, CA:false
keyUsage = critical, digitalSignature
extendedKeyUsage = critical, codeSigning
subjectKeyIdentifier = hash
EOF
# codesign refuses an expired certificate: twenty years.
"$openssl" req -x509 -newkey rsa:2048 -nodes -days 7300 -config "$tmp/cert.cnf" \
  -keyout "$tmp/key.pem" -out "$tmp/cert.pem"
# The openssl of Git for Windows ends its lines with CR LF.
"$openssl" rand -hex 16 | tr -d '\r\n' > "$password"
# The keychain of macOS imports only the old PKCS#12 algorithms; OpenSSL 3 defaults to newer ones.
mac=
case "$("$openssl" version)" in
  OpenSSL\ [3-9]*) mac="-macalg sha1" ;;
esac
# shellcheck disable=SC2086 # empty or two words
SHOT_SIGN_PASSWORD=$(cat "$password") "$openssl" pkcs12 -export -inkey "$tmp/key.pem" \
  -in "$tmp/cert.pem" -name "Shot self-signed" -certpbe PBE-SHA1-3DES -keypbe PBE-SHA1-3DES $mac \
  -passout env:SHOT_SIGN_PASSWORD -out "$p12"

leaf=$("$openssl" x509 -in "$tmp/cert.pem" -noout -fingerprint -sha1 | sed 's/.*=//; s/://g' | tr 'A-F' 'a-f')
echo "$p12: certificate leaf = H\"$leaf\""
if [ "$upload" = yes ]; then
  "$openssl" base64 -A -in "$p12" | gh secret set MAC_SIGN_P12
  gh secret set MAC_SIGN_PASSWORD < "$password"
  echo "stored as the secrets MAC_SIGN_P12 and MAC_SIGN_PASSWORD"
fi
