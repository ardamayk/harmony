#!/bin/sh
set -e
# A volume from an image that ran as root, or a restored backup, is still root's.
find /data ! -user harmony -exec chown harmony:harmony {} +
exec su-exec harmony /app/server "$@"
