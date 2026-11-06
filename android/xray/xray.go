// Package xray runs Xray inside the Android app.
//
// gomobile binds it into an AAR. Two instances may run. The tunnel: the VPN
// service establishes the TUN interface and passes its descriptor to Start,
// and the sockets Xray opens to the node go to the service's Protector, so
// they leave beside the TUN instead of into it. The relay (StartRelay): local
// entries to Control Planes' fronts, for networks that block the Control
// Planes themselves; its sockets are not protected, so while the tunnel is up
// they travel through it like the app's own.
package xray

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"sync"
	"syscall"

	"github.com/xtls/xray-core/common/platform"
	"github.com/xtls/xray-core/core"
	"github.com/xtls/xray-core/features/stats"
	"github.com/xtls/xray-core/infra/conf/serial"
	_ "github.com/xtls/xray-core/main/distro/all"
	"github.com/xtls/xray-core/transport/internet"
)

// Protector keeps a socket out of the VPN: VpnService.protect.
type Protector interface {
	Protect(fd int) bool
}

var (
	mu       sync.Mutex
	instance *core.Instance
	relay    *core.Instance
	hooked   bool
	// node is the address the tunnel's sockets go to, "ip:port"; only those
	// are protected.
	node string
)

// SetProtector hands the tunnel's sockets to the node to p. Xray keeps the
// hooks for the life of the process, and every instance shares them, so they
// are set once and pick the node's sockets out by address.
func SetProtector(p Protector) error {
	mu.Lock()
	defer mu.Unlock()
	if hooked {
		return nil
	}
	protect := func(_, address string, c syscall.RawConn) error {
		mu.Lock()
		target := node
		mu.Unlock()
		if address != target {
			return nil
		}
		var refused error
		err := c.Control(func(fd uintptr) {
			if !p.Protect(int(fd)) {
				refused = fmt.Errorf("the VPN service refused to protect a socket")
			}
		})
		if err != nil {
			return err
		}
		return refused
	}
	if err := internet.RegisterDialerController(protect); err != nil {
		return err
	}
	hooked = true
	return nil
}

// Start runs the tunnel on config, reading packets from the TUN descriptor
// fd, to the node at nodeAddress ("ip:port"), in place of any tunnel already
// running. The descriptor stays the caller's: Xray reads a duplicate and
// closes only that.
func Start(config string, fd int, nodeAddress string) error {
	mu.Lock()
	defer mu.Unlock()
	stopLocked()
	node = nodeAddress

	if err := os.Setenv(platform.TunFdKey, strconv.Itoa(fd)); err != nil {
		return err
	}
	parsed, err := serial.LoadJSONConfig(strings.NewReader(config))
	if err != nil {
		return fmt.Errorf("the configuration: %w", err)
	}
	inst, err := core.New(parsed)
	if err != nil {
		return err
	}
	if err := inst.Start(); err != nil {
		_ = inst.Close()
		return err
	}
	instance = inst
	return nil
}

// Stop stops Xray, if it runs.
func Stop() {
	mu.Lock()
	defer mu.Unlock()
	stopLocked()
}

func stopLocked() {
	if instance != nil {
		_ = instance.Close()
		instance = nil
	}
}

// StartRelay runs the relay on config, in place of any relay running. It
// needs no VPN: what it reaches is Control Planes' fronts.
func StartRelay(config string) error {
	mu.Lock()
	defer mu.Unlock()
	if relay != nil {
		_ = relay.Close()
		relay = nil
	}
	parsed, err := serial.LoadJSONConfig(strings.NewReader(config))
	if err != nil {
		return fmt.Errorf("the relay configuration: %w", err)
	}
	inst, err := core.New(parsed)
	if err != nil {
		return err
	}
	if err := inst.Start(); err != nil {
		_ = inst.Close()
		return err
	}
	relay = inst
	return nil
}

// StopRelay stops the relay, if it runs.
func StopRelay() {
	mu.Lock()
	defer mu.Unlock()
	if relay != nil {
		_ = relay.Close()
		relay = nil
	}
}

// Running reports whether the tunnel runs.
func Running() bool {
	mu.Lock()
	defer mu.Unlock()
	return instance != nil
}

// Uplink is the bytes the device has sent into the tunnel since Start.
func Uplink() int64 { return counter("uplink") }

// Downlink is the bytes the device has received from the tunnel since
// Start.
func Downlink() int64 { return counter("downlink") }

func counter(direction string) int64 {
	mu.Lock()
	defer mu.Unlock()
	if instance == nil {
		return 0
	}
	manager, ok := instance.GetFeature(stats.ManagerType()).(stats.Manager)
	if !ok {
		return 0
	}
	c := manager.GetCounter("inbound>>>tun>>>traffic>>>" + direction)
	if c == nil {
		return 0
	}
	return c.Value()
}

// Version is Xray's version.
func Version() string {
	return core.Version()
}
