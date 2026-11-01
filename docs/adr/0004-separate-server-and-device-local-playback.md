# Separate server and device-local playback

> Superseded in part by [ADR 0040](0040-shared-playback-state-with-one-active-device.md): where the Track and playhead are is now the Music Server's, and one Active Device produces sound. The separately deployed server, and that it never plays audio, remain in force.

Earthly Audio uses a separately deployed Music Server as the authority for library content and user-shared data. Web and Desktop Clients connect to that server but own their Playback Sessions and produce sound on their own devices; the Desktop Client does not bundle a server, and the Music Server does not play audio on behalf of clients. This keeps self-hosted deployment independent from client distribution while allowing browser playback and native desktop playback to share the same library and Queue.

## Considered Options

- Bundle and start the Music Server inside the Desktop Client: rejected because it creates a second server lifecycle and data ownership mode.
- Play audio on the Music Server: rejected because remote clients need sound on their own devices.
