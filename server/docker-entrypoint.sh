#!/bin/sh
set -e
# A volume from an image that ran as root, or a restored backup, is still root's.
find /data ! -user harmony -exec chown harmony:harmony {} +
# An operator's yt-dlp in the data volume outlives the image (ADR 0032).
if [ -z "$YT_DLP" ] && [ -x /data/yt-dlp/yt-dlp ]; then
	export YT_DLP=/data/yt-dlp/yt-dlp
fi
exec su-exec harmony /app/server "$@"
