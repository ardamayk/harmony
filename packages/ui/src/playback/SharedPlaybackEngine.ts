import type { PlaybackState, PlaybackStateCommand } from "@repo/api-client";
import type {
	PlaybackEngine,
	PlaybackSessionListener,
	PlaybackSessionState,
	PlaybackSource,
	PlaybackStatus,
} from "./PlaybackEngine";
import type {
	EqualizerPreset,
	ProcessingProfile,
	ReplayGainMode,
} from "./processing";
import {
	expectedPlayheadSeconds,
	isSnapshotLive,
	SHARED_PLAYBACK_HEARTBEAT_MS,
	SHARED_PLAYBACK_JUMP_SECONDS,
	snapshotClockOffsetMs,
} from "./shared-playback";

/**
 * Everything the engine needs from the Shared Playback State, so the same
 * wrapper serves the browser's direct HTTP client and the Desktop's bridge
 * (ADR 0040).
 */
export type SharedPlaybackTransport = {
	getDevice(): Promise<{ id: string; name: string }>;
	getState(): Promise<PlaybackState>;
	sendCommand(command: PlaybackStateCommand): Promise<PlaybackState>;
	subscribe(onState: (state: PlaybackState) => void): () => void;
	/** Null for a Track that no longer exists. */
	resolveSource(state: PlaybackState): Promise<PlaybackSource | null>;
};

type EngineMode = "dormant" | "active" | "following";

type SourceResolution = {
	key: string;
	source: PlaybackSource | null;
	done: Promise<PlaybackSource | null>;
};

const HANDSHAKE_RETRY_INITIAL_MS = 1000;
const HANDSHAKE_RETRY_MAX_MS = 30000;
const FOLLOW_TICK_MS = 1000;
const RESTORED_DRIFT_SECONDS = 2;

/**
 * One PlaybackEngine over the local one. While this device is the Active
 * Device everything passes through to the local Player, which reports to
 * and obeys the Shared Playback State through this class (the Desktop core
 * does both itself). Otherwise the device is Following: the Player is
 * silent, the session mirrors the shared snapshot, and controls become
 * commands.
 *
 * It starts suspended, so it is a plain pass-through until `PlaybackProvider`
 * says this client is neither in a Room nor a Guest, and stays one for good
 * when `isSupported` answers false (an older server, the Guest Listener).
 * `syncQueueContext` and `subscribeNavigation` mirror the local engine, since
 * their presence decides who advances the Queue.
 */
export class SharedPlaybackEngine implements PlaybackEngine {
	readonly syncQueueContext: PlaybackEngine["syncQueueContext"];
	readonly subscribeNavigation: PlaybackEngine["subscribeNavigation"];
	private localState: PlaybackSessionState;
	private published: PlaybackSessionState;
	private readonly listeners = new Set<PlaybackSessionListener>();
	private mode: EngineMode = "dormant";
	private suspended = true;
	private device: { id: string; name: string } | null = null;
	private snapshot: PlaybackState | null = null;
	private clockOffsetMs = 0;
	private lastSeenRevision: number | null = null;
	/** Snapshots older than the latest claim's answer lose against the claim. */
	private revisionFloor = 0;
	/** While a claim is out no snapshot is judged: the newest waits here. */
	private heldSnapshot: PlaybackState | null = null;
	private claimsInFlight = 0;
	private claimCount = 0;
	/**
	 * The last claim failed, so the server does not know this Player. It keeps
	 * playing regardless (ADR 0040), deaf to the state, until the next claim.
	 */
	private detached = false;
	private obeyedRemoteCommandRevision = 0;
	private carriedOutSkipRevision = 0;
	/** A source claimed paused but not loaded into the local Player yet. */
	private pendingTakeover: PlaybackSource | null = null;
	private queueSources: PlaybackSource[] = [];
	private resolution: SourceResolution | null = null;
	/** Stays until the next Track resolves, so the bar does not flash empty. */
	private followedSource: PlaybackSource | null = null;
	private lastReport: {
		atMs: number;
		positionMs: number;
		playing: boolean;
	} | null = null;
	/**
	 * A user started playback before this device knew its place in the Shared
	 * Playback State: the sound is already out here, so the handshake claims.
	 */
	private playedWhileDormant = false;
	private handshakeRetryMs = HANDSHAKE_RETRY_INITIAL_MS;
	private retryTimer: ReturnType<typeof setTimeout> | null = null;
	private followTimer: ReturnType<typeof setInterval> | null = null;
	private heartbeatTimer: ReturnType<typeof setInterval> | null = null;
	private unsubscribeTransport: (() => void) | null = null;
	private readonly unsubscribeLocal: () => void;
	private destroyed = false;

	constructor(
		private readonly local: PlaybackEngine,
		private readonly transport: SharedPlaybackTransport,
		private readonly isSupported: () => Promise<boolean> = async () => true,
	) {
		this.syncQueueContext = local.syncQueueContext?.bind(local);
		this.subscribeNavigation = local.subscribeNavigation?.bind(local);
		this.localState = local.getState();
		this.published = this.localSession();
		this.unsubscribeLocal = local.subscribe((state) =>
			this.handleLocalState(state),
		);
		void this.handshake();
	}

	getState() {
		return this.published;
	}

	subscribe(listener: PlaybackSessionListener) {
		this.listeners.add(listener);
		return () => this.listeners.delete(listener);
	}

	setSharedQueueSources(sources: PlaybackSource[]) {
		this.queueSources = sources;
	}

	async play(source?: PlaybackSource) {
		if (this.destroyed) return;
		if (!this.isShared()) {
			if (!this.suspended && source) this.playedWhileDormant = true;
			return this.local.play(source);
		}
		const target = source ?? this.pendingTakeover;
		if (target) return this.startHere(target);
		if (this.mode === "active") return this.local.play();
		return this.takeOverAtPlayhead();
	}

	unlockAudio() {
		this.local.unlockAudio?.();
	}

	previous() {
		this.navigate("previous");
	}

	next() {
		this.navigate("next");
	}

	pause() {
		if (this.isFollowing()) this.sendCommand({ type: "pause" });
		else this.local.pause();
	}

	stop() {
		// The server keeps the Track and playhead ready for the next device:
		// report the pause while the Player still names them, then clear it.
		if (this.isShared() && this.mode === "active") this.sendReport(false);
		this.local.stop();
	}

	togglePlay() {
		if (!this.isShared() || (this.mode === "active" && !this.pendingTakeover)) {
			this.local.togglePlay();
			return;
		}
		if (this.isFollowing() && this.isLive()) {
			this.pause();
			return;
		}
		void this.play().catch(() => undefined);
	}

	seek(seconds: number) {
		if (this.isFollowing()) {
			this.sendCommand({
				type: "seek",
				positionMs: Math.max(0, Math.round(seconds * 1000)),
			});
		} else if (this.pendingTakeover) {
			this.moveTakeover(seconds);
			this.sendReport();
		} else {
			this.local.seek(seconds);
		}
	}

	setVolume(value: number) {
		this.local.setVolume(value);
	}

	setPlaybackRate(rate: number) {
		this.local.setPlaybackRate?.(rate);
	}

	setStopAfterCurrent(enabled: boolean) {
		this.local.setStopAfterCurrent?.(enabled);
	}

	setTransitionFade(milliseconds: number) {
		this.local.setTransitionFade?.(milliseconds);
	}

	setGapless(enabled: boolean) {
		this.local.setGapless?.(enabled);
	}

	setProcessingProfile(profile: ProcessingProfile) {
		this.local.setProcessingProfile?.(profile);
	}

	setReplayGainMode(mode: ReplayGainMode) {
		this.local.setReplayGainMode?.(mode);
	}

	setEqualizerPreset(preset: Exclude<EqualizerPreset, "custom">) {
		this.local.setEqualizerPreset?.(preset);
	}

	setEqualizerGain(index: number, value: number) {
		this.local.setEqualizerGain?.(index, value);
	}

	refreshOutputDevices() {
		this.local.refreshOutputDevices?.();
	}

	selectDirectAlsaOutput(deviceId: string) {
		this.local.selectDirectAlsaOutput?.(deviceId);
	}

	selectExclusiveOutput() {
		this.local.selectExclusiveOutput?.();
	}

	fallbackToSystemOutput() {
		this.local.fallbackToSystemOutput?.();
	}

	enableAdaptiveSystemRate() {
		this.local.enableAdaptiveSystemRate?.();
	}

	toggleShuffle() {
		this.local.toggleShuffle();
	}

	cycleRepeatMode() {
		this.local.cycleRepeatMode();
	}

	/**
	 * Neither reports nor follows while suspended, as in a Room: everything
	 * passes through to the local Player untouched (ADR 0040).
	 */
	setSharedPlaybackSuspended(suspended: boolean) {
		// Before the guard: this wrapper starts suspended, the Desktop core does
		// not.
		this.local.setSharedPlaybackSuspended?.(suspended);
		if (this.suspended === suspended || this.destroyed) return;
		this.suspended = suspended;
		if (suspended) {
			this.mode = "dormant";
			this.playedWhileDormant = false;
			this.pendingTakeover = null;
			this.heldSnapshot = null;
			this.detached = false;
			this.lastSeenRevision = null;
			this.revisionFloor = 0;
			this.stopTimers();
			this.publishLocal();
			return;
		}
		// The state moved on while this device sat out; take it in again. A
		// failed read heals with the next frame on the stream.
		if (!this.device) return;
		this.transport
			.getState()
			.then((state) => {
				if (!this.destroyed && !this.suspended) this.adopt(state);
			})
			.catch((error) => {
				console.warn("Failed to read the shared playback state", { error });
			});
	}

	destroy() {
		this.destroyed = true;
		this.stopTimers();
		if (this.retryTimer) clearTimeout(this.retryTimer);
		this.retryTimer = null;
		this.unsubscribeTransport?.();
		this.unsubscribeTransport = null;
		this.unsubscribeLocal();
		this.listeners.clear();
		this.local.destroy();
	}

	// Shared Playback State -------------------------------------------------

	/** Whether the shared state plays any role at all right now. */
	private isShared() {
		return !this.suspended && this.mode !== "dormant";
	}

	private isFollowing() {
		return this.isShared() && this.mode === "following";
	}

	private isLive() {
		return (
			this.snapshot !== null &&
			isSnapshotLive(this.snapshot, this.clockOffsetMs, Date.now())
		);
	}

	/** The Desktop core reports and obeys itself; this wrapper only claims. */
	private localReports() {
		return this.local.reportsSharedPlaybackState === true;
	}

	private async handshake() {
		if (this.destroyed || this.device) return;
		try {
			if (!(await this.isSupported())) return;
			const [device, state] = await Promise.all([
				this.transport.getDevice(),
				this.transport.getState(),
			]);
			if (this.destroyed) return;
			this.device = device;
			this.unsubscribeTransport = this.transport.subscribe((event) =>
				this.onSnapshot(event),
			);
			if (!this.suspended) this.adopt(state);
		} catch {
			// Unreachable server: stay a plain pass-through and try again later.
			this.retryTimer = setTimeout(() => {
				this.retryTimer = null;
				void this.handshake();
			}, this.handshakeRetryMs);
			this.handshakeRetryMs = Math.min(
				this.handshakeRetryMs * 2,
				HANDSHAKE_RETRY_MAX_MS,
			);
		}
	}

	/**
	 * First contact with the server, or the first after a suspension. A
	 * restored Player (the Desktop's) stays when the state still names it on
	 * the same item; any other state wins and this Player goes silent. An
	 * empty state is claimed, so the server holds this playback again.
	 */
	private adopt(state: PlaybackState) {
		const source = this.localState.source;
		const blindStart = this.playedWhileDormant;
		this.playedWhileDormant = false;
		if (!source) {
			this.applySnapshot(state);
			return;
		}
		const isEmpty =
			state.activeDeviceId === null &&
			state.queueItemId === null &&
			state.trackId === null;
		if (blindStart || isEmpty) {
			this.becomeActive(state);
			this.claim(
				source,
				isAudible(this.localState.status),
				this.localState.currentTime * 1000,
			);
			this.publishLocal();
			return;
		}
		// Radio and the like are never named in the state, only its device.
		if (state.activeDeviceId === this.device?.id && source.type !== "track") {
			this.becomeActive(state);
			this.publishLocal();
			return;
		}
		if (
			state.activeDeviceId === this.device?.id &&
			isSameSharedItem(source, state)
		) {
			this.becomeActive(state);
			const expected = this.expectedSeconds();
			if (
				Math.abs(this.localState.currentTime - expected) >
				RESTORED_DRIFT_SECONDS
			) {
				this.local.seek(expected);
			}
			this.publishLocal();
			return;
		}
		this.remember(state);
		this.local.stop();
		this.enterFollowing();
	}

	private onSnapshot(state: PlaybackState) {
		if (this.destroyed || this.suspended || this.detached || !this.device) {
			return;
		}
		if (this.claimsInFlight > 0) {
			this.heldSnapshot = state;
			return;
		}
		if (this.mode === "dormant") {
			this.adopt(state);
			return;
		}
		if (
			state.revision < this.revisionFloor ||
			(this.lastSeenRevision !== null &&
				state.revision <= this.lastSeenRevision)
		) {
			return;
		}
		this.applySnapshot(state);
	}

	private applySnapshot(state: PlaybackState) {
		this.remember(state);
		const isActiveHere =
			state.activeDeviceId === this.device?.id &&
			(this.localState.source !== null || this.pendingTakeover !== null);
		if (!isActiveHere) {
			this.enterFollowing();
			return;
		}
		if (this.mode === "active") this.obey(state);
		else this.becomeActive(state);
		this.publishLocal();
	}

	private remember(state: PlaybackState) {
		this.snapshot = state;
		this.clockOffsetMs = snapshotClockOffsetMs(state, Date.now());
		this.lastSeenRevision = Math.max(
			this.lastSeenRevision ?? 0,
			state.revision,
		);
	}

	/**
	 * Turns this device into the Active Device as of a snapshot, taking the
	 * command revisions in it as already handled: only later ones are
	 * obeyed, so a resumed session never replays old commands.
	 */
	private becomeActive(state: PlaybackState) {
		this.remember(state);
		this.obeyedRemoteCommandRevision = state.remoteCommandRevision;
		this.carriedOutSkipRevision = state.skipRequest?.revision ?? 0;
		this.enterActive();
	}

	private enterActive() {
		this.mode = "active";
		this.stopFollowTimer();
		if (!this.heartbeatTimer) {
			this.heartbeatTimer = setInterval(() => {
				if (!isAudible(this.localState.status)) return;
				if (this.detached) this.retryClaim();
				else this.sendReport();
			}, SHARED_PLAYBACK_HEARTBEAT_MS);
		}
	}

	/**
	 * The claim failed, so the others do not know of this Player. Claiming
	 * blindly could take the sound back from a device that started meanwhile,
	 * so only an unchanged Active Device is claimed over.
	 */
	private retryClaim() {
		this.transport
			.getState()
			.then((state) => {
				if (this.destroyed || this.suspended || !this.detached) return;
				if (state.activeDeviceId === this.snapshot?.activeDeviceId) {
					this.claim(
						this.localState.source,
						isAudible(this.localState.status),
						this.localState.currentTime * 1000,
					);
				} else {
					this.detached = false;
					this.applySnapshot(state);
				}
			})
			.catch((error) => {
				console.warn("Failed to read the shared playback state", { error });
			});
	}

	private enterFollowing() {
		this.mode = "following";
		this.pendingTakeover = null;
		// The Desktop core silences itself.
		if (this.localState.source && !this.localReports()) this.local.stop();
		this.stopHeartbeat();
		if (!this.followTimer) {
			this.followTimer = setInterval(
				() => this.publishFollowing(),
				FOLLOW_TICK_MS,
			);
		}
		if (this.snapshot) this.resolve(this.snapshot);
		this.publishFollowing();
	}

	/** The Active Device acts on each newer snapshot. */
	private obey(state: PlaybackState) {
		if (this.localReports()) return;
		if (state.remoteCommandRevision > this.obeyedRemoteCommandRevision) {
			this.obeyedRemoteCommandRevision = state.remoteCommandRevision;
			if (!state.playing && isAudible(this.localState.status)) {
				this.local.pause();
			}
			const expected = this.expectedSeconds();
			if (
				Math.abs(this.localSeconds() - expected) > SHARED_PLAYBACK_JUMP_SECONDS
			) {
				if (this.pendingTakeover) this.moveTakeover(expected);
				else this.local.seek(expected);
			}
		}
		const skip = state.skipRequest;
		if (skip && skip.revision > this.carriedOutSkipRevision) {
			this.carriedOutSkipRevision = skip.revision;
			if (this.pendingTakeover) this.takeOverNeighbour(skip.direction);
			else this.local[skip.direction]();
		}
	}

	/**
	 * Sends the claim that makes this device the Active Device. Until it is
	 * answered no snapshot is judged and no report goes out: both still speak
	 * of the previous Active Device, and either would silence this Player.
	 */
	private claim(
		source: PlaybackSource | null,
		playing: boolean,
		positionMs: number,
	) {
		if (!this.device) return;
		const command: PlaybackStateCommand = {
			type: "claim",
			deviceId: this.device.id,
			deviceName: this.device.name,
			queueItemId: sharedQueueItemId(source),
			trackId: sharedTrackId(source),
			positionMs: Math.max(0, Math.round(positionMs)),
			playing,
		};
		this.noteReported(command.positionMs, playing);
		this.detached = false;
		this.claimsInFlight += 1;
		const claimNumber = ++this.claimCount;
		this.transport
			.sendCommand(command)
			.then((state) => {
				this.claimsInFlight -= 1;
				if (this.destroyed || this.suspended) return;
				if (state.revision >= this.revisionFloor) {
					this.revisionFloor = state.revision;
					this.becomeActive(state);
				}
				if (this.claimsInFlight > 0) return;
				const held = this.heldSnapshot;
				this.heldSnapshot = null;
				if (held) this.onSnapshot(held);
				// A blocked start claimed playing but never made a sound.
				if (isAudible(this.localState.status) !== this.snapshot?.playing) {
					this.sendReport();
				}
			})
			.catch((error) => {
				this.claimsInFlight -= 1;
				if (claimNumber === this.claimCount) {
					this.detached = true;
					this.heldSnapshot = null;
				}
				console.warn("Shared playback claim failed; playing on regardless", {
					error,
				});
			});
	}

	/**
	 * Starting playback is optimistic: browsers allow play() only inside the
	 * user gesture, so the local Player starts in this same tick, and the
	 * claim is never awaited. If it fails this device keeps playing (ADR 0040).
	 */
	private startHere(source: PlaybackSource) {
		this.pendingTakeover = null;
		this.enterActive();
		this.claim(
			source,
			true,
			source.type === "track" ? (source.startPositionSeconds ?? 0) * 1000 : 0,
		);
		return this.local.play(source);
	}

	private navigate(direction: "next" | "previous") {
		if (!this.isShared() || (this.mode === "active" && !this.pendingTakeover)) {
			this.local[direction]();
		} else if (this.isFollowing() && this.isLive()) {
			// The Active Device moves and reports; the sound stays where it is.
			this.sendCommand({ type: "skip", direction });
		} else {
			this.takeOverNeighbour(direction);
		}
	}

	/** Nothing is playing anywhere, so this device moves there, paused. */
	private takeOverNeighbour(direction: "next" | "previous") {
		const currentId =
			sharedQueueItemId(this.pendingTakeover) ?? this.snapshot?.queueItemId;
		const index = this.queueSources.findIndex(
			(source) => source.type === "track" && source.queueItemId === currentId,
		);
		const neighbour =
			index < 0
				? undefined
				: this.queueSources[index + (direction === "next" ? 1 : -1)];
		if (!neighbour) return;
		this.pendingTakeover = neighbour;
		this.enterActive();
		this.claim(neighbour, false, 0);
		this.publishLocal();
	}

	private moveTakeover(seconds: number) {
		if (this.pendingTakeover?.type !== "track") return;
		this.pendingTakeover = {
			...this.pendingTakeover,
			startPositionSeconds: seconds,
		};
		this.publishLocal();
	}

	private async takeOverAtPlayhead() {
		const state = this.snapshot;
		// Nothing to take over: a following Player never starts by itself.
		if (!state || (state.queueItemId === null && state.trackId === null)) {
			return;
		}
		const resolution = this.resolve(state);
		// Without a wait when the Track is already known, inside the gesture.
		const source = resolution.source ?? (await resolution.done);
		if (source?.type !== "track" || this.destroyed || !this.isFollowing()) {
			return;
		}
		const expected = this.expectedSeconds();
		// A Track that played to its end starts over, or it would end at once.
		const atEnd = expected > source.track.durationMs / 1000 - 1;
		return this.startHere({
			...source,
			startPositionSeconds: atEnd ? 0 : expected,
		});
	}

	private expectedSeconds() {
		if (!this.snapshot) return 0;
		return expectedPlayheadSeconds(
			this.snapshot,
			this.clockOffsetMs,
			Date.now(),
		);
	}

	private localSeconds() {
		const pending = this.pendingTakeover;
		if (pending?.type === "track") return pending.startPositionSeconds ?? 0;
		return this.localState.currentTime;
	}

	/** Finds the snapshot's Track once; the latest item asked for wins. */
	private resolve(state: PlaybackState): SourceResolution {
		const key = `${state.queueItemId}/${state.trackId}`;
		if (this.resolution?.key === key) return this.resolution;
		const resolution: SourceResolution = {
			key,
			source: null,
			done: Promise.resolve(null),
		};
		this.resolution = resolution;
		resolution.done = this.transport
			.resolveSource(state)
			.catch((error) => {
				console.warn("Failed to resolve the shared playback source", { error });
				// Asked again with the next snapshot: the failure may have passed.
				if (this.resolution === resolution) this.resolution = null;
				return null;
			})
			.then((source) => {
				if (this.destroyed || this.resolution !== resolution) return source;
				resolution.source = source;
				this.followedSource = source;
				this.publishFollowing();
				return source;
			});
		return resolution;
	}

	// Reporting -------------------------------------------------------------

	/** Reports go out immediately on play/pause change, source change and jumps. */
	private considerReport(
		previous: PlaybackSessionState,
		next: PlaybackSessionState,
	) {
		// With nothing loaded there is nothing to report: `stop` said its last.
		if (this.mode !== "active" || !next.source || !this.lastReport) return;
		if (
			isAudible(next.status) !== isAudible(previous.status) ||
			next.source !== previous.source
		) {
			this.sendReport();
			return;
		}
		const { atMs, positionMs, playing } = this.lastReport;
		const expectedMs = positionMs + (playing ? Date.now() - atMs : 0);
		if (
			Math.abs(next.currentTime * 1000 - expectedMs) >
			SHARED_PLAYBACK_JUMP_SECONDS * 1000
		) {
			this.sendReport();
		}
	}

	private noteReported(positionMs: number, playing: boolean) {
		this.lastReport = { atMs: Date.now(), positionMs, playing };
	}

	private sendReport(playing = isAudible(this.localState.status)) {
		if (
			!this.device ||
			this.mode !== "active" ||
			this.claimsInFlight > 0 ||
			this.detached ||
			this.localReports()
		) {
			return;
		}
		const source = this.localState.source ?? this.pendingTakeover;
		const positionMs = Math.max(0, Math.round(this.localSeconds() * 1000));
		this.noteReported(positionMs, playing);
		// The answer can carry a remote command this device has not seen (a
		// report based on an older revision changes nothing), or another
		// Active Device, so it is judged like any snapshot.
		this.sendCommand({
			type: "report",
			deviceId: this.device.id,
			basedOnRevision: this.lastSeenRevision ?? 0,
			queueItemId: sharedQueueItemId(source),
			trackId: sharedTrackId(source),
			positionMs,
			playing,
		});
	}

	private sendCommand(command: PlaybackStateCommand) {
		this.transport
			.sendCommand(command)
			.then((state) => this.onSnapshot(state))
			.catch((error) => {
				console.warn("Shared playback command failed", { error });
			});
	}

	// Local engine events ---------------------------------------------------

	private handleLocalState(state: PlaybackSessionState) {
		if (this.destroyed) return;
		const previous = this.localState;
		this.localState = state;
		if (state.source) this.pendingTakeover = null;
		else this.detached = false;
		// The Desktop's Player can load after the snapshot naming this device.
		const snapshot = this.snapshot;
		if (
			this.isFollowing() &&
			state.source &&
			snapshot?.activeDeviceId === this.device?.id
		) {
			if (snapshot) this.becomeActive(snapshot);
		}
		if (this.isFollowing()) {
			this.publishFollowing(true);
			return;
		}
		this.publishLocal();
		if (this.isShared()) this.considerReport(previous, state);
	}

	// Published session state -----------------------------------------------

	private localSession(): PlaybackSessionState {
		return { ...this.localState, sharedPlayback: undefined };
	}

	private publish(state: PlaybackSessionState) {
		this.published = state;
		for (const listener of this.listeners) listener(state);
	}

	private publishLocal() {
		const pending = this.pendingTakeover;
		this.publish(
			pending && !this.localState.source
				? {
						...this.silentSession(pending),
						status: "paused",
						currentTime: this.localSeconds(),
						sharedPlayback: undefined,
					}
				: this.localSession(),
		);
	}

	private publishFollowing(force = false) {
		if (!this.snapshot || !this.isFollowing()) return;
		const isLive = this.isLive();
		const source = this.followedSource;
		const next: PlaybackSessionState = {
			...this.silentSession(source),
			// A following Player never shows ended or error: it is silent.
			status: source ? (isLive ? "playing" : "paused") : "idle",
			currentTime: this.expectedSeconds(),
			sharedPlayback: {
				followingDeviceName:
					isLive && this.snapshot.activeDeviceId !== this.device?.id
						? this.snapshot.activeDeviceName
						: null,
			},
		};
		// A paused playhead stands still: keep the session identity, so the UI
		// does not render every tick.
		if (
			force ||
			next.currentTime !== this.published.currentTime ||
			next.status !== this.published.status ||
			next.source !== this.published.source ||
			next.sharedPlayback?.followingDeviceName !==
				this.published.sharedPlayback?.followingDeviceName
		) {
			this.publish(next);
		}
	}

	/** The local Player's settings around a source it does not play. */
	private silentSession(source: PlaybackSource | null): PlaybackSessionState {
		return {
			...this.localState,
			source,
			duration: sourceDurationSeconds(source),
			bufferedEnd: null,
			error: null,
			processing: undefined,
			telemetry: undefined,
		};
	}

	private stopHeartbeat() {
		if (this.heartbeatTimer) clearInterval(this.heartbeatTimer);
		this.heartbeatTimer = null;
	}

	private stopFollowTimer() {
		if (this.followTimer) clearInterval(this.followTimer);
		this.followTimer = null;
	}

	private stopTimers() {
		this.stopHeartbeat();
		this.stopFollowTimer();
	}
}

function isAudible(status: PlaybackStatus) {
	return status === "playing" || status === "reconnecting";
}

/** The Queue item a Track source plays, or null for any other source. */
function sharedQueueItemId(source: PlaybackSource | null) {
	return source?.type === "track" ? (source.queueItemId ?? null) : null;
}

function sharedTrackId(source: PlaybackSource | null) {
	return source?.type === "track" ? source.track.id : null;
}

/** Whether a restored Player plays the item the shared state names. */
function isSameSharedItem(source: PlaybackSource, state: PlaybackState) {
	if (source.type !== "track") return false;
	return source.queueItemId
		? source.queueItemId === state.queueItemId
		: source.track.id === state.trackId;
}

function sourceDurationSeconds(source: PlaybackSource | null) {
	const durationMs =
		source?.type === "track"
			? source.track.durationMs
			: source?.type === "external-track"
				? source.externalTrack.durationMs
				: null;
	return durationMs && durationMs > 0 ? durationMs / 1000 : 0;
}
