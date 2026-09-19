package configgen

const configTemplate = `mixed-port:
tproxy-port:
allow-lan: true
ipv6: true
bind-address: '*'
mode: rule
log-level: silent
unified-delay: true
external-controller: ''
external-controller-unix: '/var/apps/Fluxor/target/core.sock'
external-ui:
external-ui-url:
secret: ''

routing-mark: 255
find-process-mode: strict
client-fingerprint: chrome

profile:
  store-selected: true
  store-fake-ip: true
  smart-collector-size: 100

sniffer:
  enable: true
  sniff:
    HTTP:
      ports: [80, 8080-8880]
      override-destination: true
    TLS:
      ports: [443, 8443]
    QUIC:
      ports: [443, 8443]
  skip-domain:
    - "+.push.apple.com"

tun:
  enable: false
  stack: mixed
  dns-hijack:
    - "any:53"
    - "tcp://any:53"
  auto-route: true
  auto-redirect: true
  auto-detect-interface: true

geodata-mode: false
geo-auto-update: true
geo-update-interval: 24
`
