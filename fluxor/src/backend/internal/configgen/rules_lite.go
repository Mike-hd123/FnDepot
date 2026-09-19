package configgen

const rulesBase = `
rules:
  - GEOIP,lan,🎯 全球直连,no-resolve
  - GEOSITE,github,🚀 节点选择
  - GEOSITE,google,🚀 节点选择
  - GEOSITE,telegram,🚀 节点选择
  - GEOSITE,CN,🎯 全球直连
  - GEOSITE,geolocation-!cn,🚀 节点选择
  - GEOIP,google,🚀 节点选择
  - GEOIP,telegram,🚀 节点选择
  - GEOIP,CN,🎯 全球直连
  - MATCH,🐟 漏网之鱼
`
