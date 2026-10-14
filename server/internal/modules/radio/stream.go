package radio

import (
	"bufio"
	"context"
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/ardam/navidrome-replacement/server/internal/api/respond"
)

const nowPlayingStaleAfter = 60 * time.Second

const hlsContentType = "application/vnd.apple.mpegurl"

const maxHLSPlaylistBytes = 1 << 20

const hlsResourceTokenTTL = time.Hour

const hlsSigningKeyBytes = 32

var errUnsafeStreamURL = errors.New("unsafe stream URL")

var errUnsupportedHLSDRM = errors.New("DRM-protected HLS is unsupported")

func ValidateStreamURL(rawURL string) error {
	parsed, err := url.Parse(rawURL)
	if err != nil {
		return err
	}
	if parsed.Scheme != "http" && parsed.Scheme != "https" {
		return fmt.Errorf("%w: unsupported scheme", errUnsafeStreamURL)
	}
	host := strings.TrimSuffix(strings.ToLower(parsed.Hostname()), ".")
	if host == "" || host == "localhost" {
		return fmt.Errorf("%w: blocked host", errUnsafeStreamURL)
	}
	if ip := net.ParseIP(host); ip != nil && isBlockedIP(ip) {
		return fmt.Errorf("%w: blocked IP", errUnsafeStreamURL)
	}
	return nil
}

func isBlockedIP(ip net.IP) bool {
	return ip.IsLoopback() || ip.IsPrivate() || ip.IsLinkLocalUnicast() || ip.IsLinkLocalMulticast() || ip.IsUnspecified()
}

type NowPlayingCache struct {
	mu    sync.RWMutex
	items map[string]NowPlaying
}

func NewNowPlayingCache() *NowPlayingCache {
	return &NowPlayingCache{items: make(map[string]NowPlaying)}
}

func (c *NowPlayingCache) Set(stationID string, now NowPlaying) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.items[stationID] = now
}

func (c *NowPlayingCache) Get(stationID string) (NowPlaying, bool) {
	c.mu.RLock()
	defer c.mu.RUnlock()
	now, ok := c.items[stationID]
	if !ok {
		return NowPlaying{}, false
	}
	if now.UpdatedAt != nil && time.Since(*now.UpdatedAt) > nowPlayingStaleAfter {
		now.Stale = true
	}
	return now, true
}

type ICYReader struct {
	source    *bufio.Reader
	closer    io.Closer
	metaint   int
	remaining int
	onUpdate  func(NowPlaying)
}

func NewICYReader(source io.ReadCloser, metaint int, onUpdate func(NowPlaying)) *ICYReader {
	return &ICYReader{
		source:    bufio.NewReader(source),
		closer:    source,
		metaint:   metaint,
		remaining: metaint,
		onUpdate:  onUpdate,
	}
}

func (r *ICYReader) Read(p []byte) (int, error) {
	if r.metaint <= 0 {
		return r.source.Read(p)
	}
	if r.remaining == 0 {
		if err := r.readMetadata(); err != nil {
			return 0, err
		}
		r.remaining = r.metaint
	}
	if len(p) > r.remaining {
		p = p[:r.remaining]
	}
	n, err := r.source.Read(p)
	r.remaining -= n
	return n, err
}

func (r *ICYReader) Close() error {
	return r.closer.Close()
}

func (r *ICYReader) readMetadata() error {
	lengthByte, err := r.source.ReadByte()
	if err != nil {
		return err
	}
	length := int(lengthByte) * 16
	if length == 0 {
		return nil
	}
	data := make([]byte, length)
	if _, err := io.ReadFull(r.source, data); err != nil {
		return err
	}
	if now, ok := ParseICYMetadata(string(data)); ok && r.onUpdate != nil {
		r.onUpdate(now)
	}
	return nil
}

func ParseICYMetadata(raw string) (NowPlaying, bool) {
	raw = strings.Trim(raw, "\x00 ")
	if raw == "" {
		return NowPlaying{}, false
	}
	title := parseStreamTitle(raw)
	if title == "" {
		return NowPlaying{Raw: raw}, true
	}
	artist, trackTitle := splitArtistTitle(title)
	now := time.Now().UTC()
	return NowPlaying{
		Title:     trackTitle,
		Artist:    artist,
		Raw:       title,
		UpdatedAt: &now,
	}, true
}

func parseStreamTitle(raw string) string {
	const prefix = "StreamTitle='"
	start := strings.Index(raw, prefix)
	if start < 0 {
		return ""
	}
	start += len(prefix)
	end := strings.Index(raw[start:], "';")
	if end < 0 {
		return strings.TrimSpace(raw[start:])
	}
	return strings.TrimSpace(raw[start : start+end])
}

func splitArtistTitle(raw string) (artist string, title string) {
	parts := strings.SplitN(raw, " - ", 2)
	if len(parts) != 2 {
		return "", raw
	}
	return strings.TrimSpace(parts[0]), strings.TrimSpace(parts[1])
}

type StreamProxy struct {
	client     *http.Client
	cache      *NowPlayingCache
	stations   *Service
	signingKey []byte
	now        func() time.Time
}

func NewStreamProxy(stations *Service, cache *NowPlayingCache) *StreamProxy {
	client := &http.Client{
		Transport: newSecureStreamTransport(),
		Timeout:   0,
		CheckRedirect: func(req *http.Request, via []*http.Request) error {
			if len(via) >= 5 {
				return errors.New("too many redirects")
			}
			return ValidateStreamURL(req.URL.String())
		},
	}
	return &StreamProxy{
		client:     client,
		cache:      cache,
		stations:   stations,
		signingKey: generateHLSSigningKey(),
		now:        time.Now,
	}
}

func generateHLSSigningKey() []byte {
	key := make([]byte, hlsSigningKeyBytes)
	if _, err := rand.Read(key); err != nil {
		panic(fmt.Sprintf("generate HLS signing key: %v", err))
	}
	return key
}

func newSecureStreamTransport() *http.Transport {
	return newSecureStreamTransportWithResolver(net.DefaultResolver.LookupIPAddr)
}

func newSecureStreamTransportWithResolver(lookupIPAddresses func(context.Context, string) ([]net.IPAddr, error)) *http.Transport {
	transport := http.DefaultTransport.(*http.Transport).Clone()
	transport.Proxy = nil
	dialer := &net.Dialer{Timeout: 30 * time.Second, KeepAlive: 30 * time.Second}
	transport.DialContext = func(ctx context.Context, network, address string) (net.Conn, error) {
		host, port, err := net.SplitHostPort(address)
		if err != nil {
			return nil, fmt.Errorf("invalid stream address: %w", err)
		}
		addresses, err := lookupIPAddresses(ctx, host)
		if err != nil {
			return nil, fmt.Errorf("resolve stream host: %w", err)
		}
		for _, address := range addresses {
			if isBlockedIP(address.IP) {
				return nil, fmt.Errorf("%w: resolved to blocked IP", errUnsafeStreamURL)
			}
		}
		var dialErr error
		for _, address := range addresses {
			connection, err := dialer.DialContext(ctx, network, net.JoinHostPort(address.IP.String(), port))
			if err == nil {
				return connection, nil
			}
			dialErr = err
		}
		if dialErr != nil {
			return nil, dialErr
		}
		return nil, errors.New("stream host resolved to no addresses")
	}
	return transport
}

func (p *StreamProxy) Stream(w http.ResponseWriter, r *http.Request, station Station) {
	if _, hasCallerSuppliedURL := r.URL.Query()["url"]; hasCallerSuppliedURL {
		respond.Error(w, http.StatusBadRequest, "bad_request", "invalid HLS resource token")
		return
	}
	streamURL, err := p.resolveStreamURL(r, station)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	if validationErr := ValidateStreamURL(streamURL); validationErr != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", validationErr.Error())
		return
	}
	req, err := http.NewRequestWithContext(r.Context(), http.MethodGet, streamURL, nil)
	if err != nil {
		respond.Error(w, http.StatusBadRequest, "bad_request", err.Error())
		return
	}
	req.Header.Set("Icy-MetaData", "1")
	if rangeHeader := r.Header.Get("Range"); rangeHeader != "" {
		req.Header.Set("Range", rangeHeader)
	}
	res, err := p.client.Do(req)
	if err != nil {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", err.Error())
		return
	}
	defer func() {
		_ = res.Body.Close()
	}()
	if res.StatusCode < 200 || res.StatusCode >= 300 {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", fmt.Sprintf("stream status %d", res.StatusCode))
		return
	}
	if isHLSPlaylist(res) {
		p.writeHLSPlaylist(w, res, r.URL.Path, station.ID)
		return
	}
	p.writeAudioStream(w, r, res, station)
}

func (p *StreamProxy) resolveStreamURL(r *http.Request, station Station) (string, error) {
	token := r.URL.Query().Get("resource")
	if token == "" {
		return station.StreamURL, nil
	}
	return p.verifyHLSResourceToken(token, station.ID)
}

func (p *StreamProxy) writeHLSPlaylist(w http.ResponseWriter, response *http.Response, proxyPath, stationID string) {
	playlist, err := io.ReadAll(io.LimitReader(response.Body, maxHLSPlaylistBytes+1))
	if err != nil {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", "failed to read HLS playlist")
		return
	}
	if len(playlist) > maxHLSPlaylistBytes {
		respond.Error(w, http.StatusBadGateway, "bad_gateway", "HLS playlist exceeds size limit")
		return
	}
	rewritten, err := p.rewriteHLSPlaylist(playlist, response.Request.URL, proxyPath, stationID)
	if err != nil {
		status := http.StatusBadGateway
		if errors.Is(err, errUnsupportedHLSDRM) {
			status = http.StatusUnsupportedMediaType
		}
		code := "bad_gateway"
		if status == http.StatusUnsupportedMediaType {
			code = "unsupported_media_type"
		}
		respond.Error(w, status, code, err.Error())
		return
	}
	copyStreamHeaders(w, response.Header)
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(rewritten)
}

func (p *StreamProxy) writeAudioStream(w http.ResponseWriter, r *http.Request, response *http.Response, station Station) {
	copyStreamHeaders(w, response.Header)
	w.WriteHeader(response.StatusCode)
	reader := io.ReadCloser(response.Body)
	if metaint, err := strconv.Atoi(response.Header.Get("icy-metaint")); err == nil && metaint > 0 {
		reader = NewICYReader(response.Body, metaint, func(now NowPlaying) {
			p.cache.Set(station.ID, now)
			_ = p.stations.UpdateNowPlaying(r.Context(), station.ID, now)
		})
		defer func() {
			_ = reader.Close()
		}()
	}
	written, copyErr := io.Copy(w, reader)
	if r.Context().Err() != nil || r.URL.Query().Get("resource") != "" {
		return
	}
	if copyErr != nil {
		slog.Warn("radio stream interrupted", "stationId", station.ID, "bytes", written, "error", copyErr)
		return
	}
	slog.Info("radio upstream stream ended", "stationId", station.ID, "bytes", written)
}

func isHLSPlaylist(response *http.Response) bool {
	contentType := strings.ToLower(strings.TrimSpace(strings.Split(response.Header.Get("Content-Type"), ";")[0]))
	if contentType == hlsContentType || contentType == "application/x-mpegurl" || contentType == "audio/mpegurl" || contentType == "audio/x-mpegurl" {
		return true
	}
	return strings.HasSuffix(strings.ToLower(response.Request.URL.Path), ".m3u8")
}

func (p *StreamProxy) rewriteHLSPlaylist(playlist []byte, baseURL *url.URL, proxyPath, stationID string) ([]byte, error) {
	text := string(playlist)
	if !strings.HasPrefix(strings.TrimSpace(text), "#EXTM3U") {
		return nil, errors.New("invalid HLS playlist: missing EXTM3U header")
	}
	lines := strings.Split(text, "\n")
	for index, line := range lines {
		if strings.HasPrefix(line, "#") {
			if hasUnsupportedHLSProtection(line) {
				return nil, errUnsupportedHLSDRM
			}
			rewritten, err := p.rewriteHLSURIAttributes(line, baseURL, proxyPath, stationID)
			if err != nil {
				return nil, err
			}
			lines[index] = rewritten
			continue
		}
		if strings.TrimSpace(line) == "" {
			continue
		}
		rewritten, err := p.proxyHLSReference(strings.TrimSpace(line), baseURL, proxyPath, stationID)
		if err != nil {
			return nil, err
		}
		lines[index] = rewritten
	}
	return []byte(strings.Join(lines, "\n")), nil
}

func hasUnsupportedHLSProtection(line string) bool {
	upperLine := strings.ToUpper(line)
	if !strings.HasPrefix(upperLine, "#EXT-X-KEY:") && !strings.HasPrefix(upperLine, "#EXT-X-SESSION-KEY:") {
		return false
	}
	if strings.Contains(upperLine, "METHOD=SAMPLE-AES") {
		return true
	}
	keyFormatIndex := strings.Index(upperLine, "KEYFORMAT=")
	if keyFormatIndex < 0 {
		return false
	}
	keyFormat := strings.Trim(strings.SplitN(line[keyFormatIndex+len("KEYFORMAT="):], ",", 2)[0], `" `)
	return !strings.EqualFold(keyFormat, "identity")
}

func (p *StreamProxy) rewriteHLSURIAttributes(line string, baseURL *url.URL, proxyPath, stationID string) (string, error) {
	const marker = `URI="`
	for offset := 0; ; {
		start := strings.Index(line[offset:], marker)
		if start < 0 {
			return line, nil
		}
		start += offset + len(marker)
		end := strings.IndexByte(line[start:], '"')
		if end < 0 {
			return "", errors.New("invalid HLS playlist: unterminated URI attribute")
		}
		end += start
		rewritten, err := p.proxyHLSReference(line[start:end], baseURL, proxyPath, stationID)
		if err != nil {
			return "", err
		}
		line = line[:start] + rewritten + line[end:]
		offset = start + len(rewritten) + 1
	}
}

func (p *StreamProxy) proxyHLSReference(reference string, baseURL *url.URL, proxyPath, stationID string) (string, error) {
	parsed, err := url.Parse(reference)
	if err != nil {
		return "", fmt.Errorf("invalid HLS playlist URL: %w", err)
	}
	resolved := baseURL.ResolveReference(parsed)
	if err := ValidateStreamURL(resolved.String()); err != nil {
		return "", fmt.Errorf("invalid HLS playlist URL: %w", err)
	}
	query := url.Values{"resource": []string{p.signHLSResource(stationID, resolved.String())}}
	return proxyPath + "?" + query.Encode(), nil
}

type hlsResourceClaims struct {
	StationID string `json:"s"`
	URL       string `json:"u"`
	ExpiresAt int64  `json:"e"`
}

func (p *StreamProxy) signHLSResource(stationID, resourceURL string) string {
	claims := hlsResourceClaims{
		StationID: stationID,
		URL:       resourceURL,
		ExpiresAt: p.currentTime().Add(hlsResourceTokenTTL).Unix(),
	}
	payload, _ := json.Marshal(claims)
	encodedPayload := base64.RawURLEncoding.EncodeToString(payload)
	signature := hmac.New(sha256.New, p.signingKey)
	_, _ = signature.Write([]byte(encodedPayload))
	return encodedPayload + "." + base64.RawURLEncoding.EncodeToString(signature.Sum(nil))
}

func (p *StreamProxy) verifyHLSResourceToken(token, stationID string) (string, error) {
	encodedPayload, encodedSignature, ok := strings.Cut(token, ".")
	if !ok {
		return "", errors.New("invalid HLS resource token")
	}
	signature, err := base64.RawURLEncoding.DecodeString(encodedSignature)
	if err != nil {
		return "", errors.New("invalid HLS resource token")
	}
	expected := hmac.New(sha256.New, p.signingKey)
	_, _ = expected.Write([]byte(encodedPayload))
	if !hmac.Equal(signature, expected.Sum(nil)) {
		return "", errors.New("invalid HLS resource token")
	}
	payload, err := base64.RawURLEncoding.DecodeString(encodedPayload)
	if err != nil {
		return "", errors.New("invalid HLS resource token")
	}
	var claims hlsResourceClaims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return "", errors.New("invalid HLS resource token")
	}
	if claims.StationID != stationID || claims.ExpiresAt <= p.currentTime().Unix() {
		return "", errors.New("invalid HLS resource token")
	}
	if err := ValidateStreamURL(claims.URL); err != nil {
		return "", fmt.Errorf("invalid HLS resource token: %w", err)
	}
	return claims.URL, nil
}

func (p *StreamProxy) currentTime() time.Time {
	if p.now != nil {
		return p.now()
	}
	return time.Now()
}

func copyStreamHeaders(w http.ResponseWriter, headers http.Header) {
	for _, key := range []string{"Content-Type", "Cache-Control", "Content-Range", "Accept-Ranges", "icy-name", "icy-genre", "icy-br", "icy-url"} {
		if value := headers.Get(key); value != "" {
			w.Header().Set(key, value)
		}
	}
}
