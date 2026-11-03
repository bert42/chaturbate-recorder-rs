#!/bin/bash
# Check which hosts the Chaturbate video CDN (mmcdn) accepts.
# usage: cdn-probe.sh -r room host1 host2 ...     (use "local" for this machine)
#
# Per host:
#   edge_root  GET https://<edge>/  -> 200 = IP allowed, 403 = IP blocked (no cookies/token needed)
#   site       chaturbate API from that host -> ok, or "challenged" (Cloudflare wants cookies)
#   stream     when site is ok: real master playlist fetched with a token minted for that host
ROOM=
[ "$1" = "-r" ] && { ROOM=$2; shift 2; }
[ -z "$ROOM" ] || [ $# -eq 0 ] && { echo "usage: $0 -r <live_room> host... (\"local\" = this machine)" >&2; exit 2; }
EDGE=${CDN_EDGE:-edge30-waw.live.mmcdn.com}   # any edge host from a real stream URL

read -r -d '' REMOTE <<EOF
UA='Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/130.0.0.0 Safari/537.36'
ip=\$(curl -s -4 --max-time 5 https://ifconfig.me || echo ?)
root=\$(curl -s -o /dev/null --max-time 10 -w '%{http_code}' https://$EDGE/)
api=\$(curl -s --max-time 10 -A "\$UA" https://chaturbate.com/api/chatvideocontext/$ROOM/)
hls=\$(printf '%s' "\$api" | grep -o '"hls_source": *"[^"]*"' | sed 's/.*: *"//; s/"\$//')
if printf '%s' "\$api" | grep -q '"room_status"'; then
  site=ok
  if [ -n "\$hls" ]; then stream=\$(curl -s -o /dev/null --max-time 10 -A "\$UA" -w '%{http_code}' "\$hls"); else stream="(room offline)"; fi
else
  site=challenged; stream=-
fi
echo "ip=\$ip edge_root=\$root site=\$site stream=\$stream"
EOF

printf '%-16s %s\n' HOST RESULT
for h in "$@"; do
  if [ "$h" = local ]; then out=$(bash -c "$REMOTE" 2>&1)
  else out=$(ssh -o BatchMode=yes -o ConnectTimeout=8 "$h" "bash -s" <<<"$REMOTE" 2>&1 | tail -1); fi
  printf '%-16s %s\n' "$h" "$out"
done
