package configgen

// DnsBlock 统一注入到 config.yaml 的 DNS 配置块（fake-ip 模式 + 国内外双解析）。
const DnsBlock = `
dns:
  enable: true
  listen: 0.0.0.0:1053
  prefer-h3: true
  ipv6: true
  use-hosts: true
  respect-rules: true
  default-nameserver:
    - https://223.5.5.5/dns-query
  enhanced-mode: fake-ip
  # 不用 mihomo 默认 198.18.0.0/16：全网客户端/路由器大量使用该段，易与
  # 第三方 fake-ip（及历史上的 easytier Meta 网卡 198.18.0.0/30）撞车。
  # 198.19.0.0/16 与 TR3000 路由侧既有「ip route 198.19.0.0/16 via NAS」
  # 全屋分流基础设施对齐，且不与 easytier(192.168.3.0/24)/上级路由
  # (192.168.19.0/16)/TProxy 私网 bypass 集（含 240.0.0.0/4）冲突。
  fake-ip-range: 198.19.0.1/16
  fake-ip-filter:
    - '*.lan'
    - '*.local'
    - '*.localhost'
    - localhost.ptlogin2.qq.com
    - '+.stun.*.*'
    - '+.stun.*.*.*'
    - '+.stun.*.*.*.*'
    - lens.l.google.com
    - '*.srv.nintendo.net'
    - +.stun.playstation.net
    - 'xbox.*.*.microsoft.com'
    - '*.*.xboxlive.com'
    - +.msftncsi.com
    - +.msftconnecttest.com
  nameserver:
    - https://120.53.53.53/dns-query
    - https://223.5.5.5/dns-query
  proxy-server-nameserver:
    - https://120.53.53.53/dns-query
    - https://223.5.5.5/dns-query
`
