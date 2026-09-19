package configgen

// proxyGroupsBaseTemplate 是 base 规则集的模板。
//
// 1.4.0-1：补齐 5 个 smart 地区组（🇭🇰 香港 / 🇹🇼 台湾 / 🇯🇵 日本 / 🇸🇬 新加坡 / 🇺🇸 美国），
// 与 full 规则集对齐。订阅名通过 __SUB_NAMES__ 占位，由 proxyGroupsBase(subs) 注入真实节点，
// 避免订阅名含 `,` / `]` 时破坏 flow sequence 语法。
const proxyGroupsBaseTemplate = `
proxy-groups:
  - {name: 🚀 节点选择, type: select, proxies: [👉 手动选择,♻️ 自动选择, 🇭🇰 香港节点, 🇹🇼 台湾节点, 🇯🇵 日本节点, 🇸🇬 新加坡节点, 🇺🇸 美国节点]}
  - {name: 👉 手动选择, type: select, include-all: true}
  - {name: 🇭🇰 香港节点, type: smart, use: [__SUB_NAMES__], filter: "(?i)(🇭🇰|港|hk|hongkong|hong kong)", interval: 300, lazy: true, url: 'https://www.google.com/generate_204', uselightgbm: true, collectdata: true, include-all: true, sample-rate: 1, prefer-asn: true, icon: "https://testingcf.jsdelivr.net/gh/Koolson/Qure@master/IconSet/Color/Hong_Kong.png"}
  - {name: 🇹🇼 台湾节点, type: smart, use: [__SUB_NAMES__], filter: "(?i)(🇹🇼|台|tw|taiwan|tai wan)", interval: 300, lazy: true, url: 'https://www.google.com/generate_204', uselightgbm: true, collectdata: true, include-all: true, sample-rate: 1, prefer-asn: true, icon: "https://testingcf.jsdelivr.net/gh/Koolson/Qure@master/IconSet/Color/Taiwan.png"}
  - {name: 🇯🇵 日本节点, type: smart, use: [__SUB_NAMES__], filter: "(?i)(🇯🇵|日|jp|japan)", interval: 300, lazy: true, url: 'https://www.google.com/generate_204', uselightgbm: true, collectdata: true, include-all: true, sample-rate: 1, prefer-asn: true, icon: "https://testingcf.jsdelivr.net/gh/Koolson/Qure@master/IconSet/Color/Japan.png"}
  - {name: 🇸🇬 新加坡节点, type: smart, use: [__SUB_NAMES__], filter: "(?i)(🇸🇬|新|sg|singapore)", interval: 300, lazy: true, url: 'https://www.google.com/generate_204', uselightgbm: true, collectdata: true, include-all: true, sample-rate: 1, prefer-asn: true, icon: "https://testingcf.jsdelivr.net/gh/Koolson/Qure@master/IconSet/Color/Singapore.png"}
  - {name: 🇺🇸 美国节点, type: smart, use: [__SUB_NAMES__], filter: "(?i)(🇺🇸|美|us|unitedstates|united states)", interval: 300, lazy: true, url: 'https://www.google.com/generate_204', uselightgbm: true, collectdata: true, include-all: true, sample-rate: 1, prefer-asn: true, icon: "https://testingcf.jsdelivr.net/gh/Koolson/Qure@master/IconSet/Color/United_States.png"}
  - {name: ♻️ 自动选择, type: url-test, include-all: true, tolerance: 100}
  - {name: 🐟 漏网之鱼, type: select, proxies: [🚀 节点选择, 🎯 全球直连]}
  - {name: 🎯 全球直连, type: select, proxies: [DIRECT], hidden: true}
`
