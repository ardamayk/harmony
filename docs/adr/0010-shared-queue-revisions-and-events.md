# Shared queue revisions and events

> Superseded in part by [ADR 0040](0040-shared-playback-state-with-one-active-device.md): where the Track and playhead are is now the Music Server's, and one Active Device produces sound. The shared Queue, its revisions and its events remain in force.

The user Queue is shared across Playback Clients, carries a Queue Revision, and is synchronized through server-sent events while each Playback Session remains device-local. A currently playing Track continues if another client removes it or clears the Queue; when it ends, the Player follows the latest Queue or stops if none remains.

Mutations based on a stale Queue Revision are rejected. Clients may refetch and retry an unambiguous append or remove once, while replace and reorder conflicts require visible user resolution. The one exception is the client's play-with-context operation (playing a Track from an album, playlist, or genre list): its replacement content does not depend on the previous Queue, so it is unambiguous and the client refetches and retries it once. Plain replace (such as clearing the Queue) still surfaces the conflict.
