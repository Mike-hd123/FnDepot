package tproxy

import "testing"

func TestIsTunnelInterface(t *testing.T) {
	tunnel := []string{"tun0", "tun1", "wg0", "wg-server", "Meta5", "ipvl0", "ipvl1"}
	for _, n := range tunnel {
		if !isTunnelInterface(n) {
			t.Errorf("isTunnelInterface(%q) = false, want true", n)
		}
	}
	normal := []string{"lo", "enp2s0", "enp4s0", "eth0", "wlp5s0", "docker0", "br-b4528bb647c9", "br0", "veth123"}
	for _, n := range normal {
		if isTunnelInterface(n) {
			t.Errorf("isTunnelInterface(%q) = true, want false", n)
		}
	}
}

func TestParseTunnelInterfaces(t *testing.T) {
	out := `1: lo: <LOOPBACK,UP,LOWER_UP> mtu 65536 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\    link/loopback 00:00:00:00:00:00 brd 00:00:00:00:00:00
2: enp2s0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UP mode DEFAULT group default qlen 1000\    link/ether 00:11:22:33:44:55 brd ff:ff:ff:ff:ff:ff
5: ipvl0@enp2s0: <BROADCAST,MULTICAST> mtu 1500 qdisc noop state DOWN mode DEFAULT group default qlen 1000\    link/ether 00:11:22:33:44:55 brd ff:ff:ff:ff:ff:ff
7: docker0: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc noqueue state UP mode DEFAULT group default\    link/ether 66:77:88:99:aa:bb brd ff:ff:ff:ff:ff:ff
12: tun0: <POINTOPOINT,MULTICAST,NOARP,UP,LOWER_UP> mtu 1500 qdisc fq_codel state UNKNOWN mode DEFAULT group default qlen 500\    link/none
13: wg1: <POINTOPOINT,NOARP,UP,LOWER_UP> mtu 1420 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\    link/none
14: Meta5: <BROADCAST,MULTICAST,UP,LOWER_UP> mtu 1500 qdisc noqueue state UNKNOWN mode DEFAULT group default qlen 1000\    link/ether aa:bb:cc:dd:ee:ff brd ff:ff:ff:ff:ff:ff
15: tun0: <POINTOPOINT> mtu 1500 qdisc noqueue state UNKNOWN\    link/none`

	got := parseTunnelInterfaces(out)
	want := []string{"ipvl0", "tun0", "wg1", "Meta5"}
	if len(got) != len(want) {
		t.Fatalf("parseTunnelInterfaces() = %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("got[%d] = %q, want %q", i, got[i], want[i])
		}
	}
}

func TestParseTunnelInterfacesEmpty(t *testing.T) {
	if got := parseTunnelInterfaces(""); got != nil {
		t.Errorf("parseTunnelInterfaces(\"\") = %v, want nil", got)
	}
	// 物理设备环境：不应产出任何隧道设备
	out := `1: lo: <LOOPBACK> mtu 65536\    link/loopback
2: eth0: <BROADCAST> mtu 1500\    link/ether`
	if got := parseTunnelInterfaces(out); got != nil {
		t.Errorf("parseTunnelInterfaces(phys-only) = %v, want nil", got)
	}
}
