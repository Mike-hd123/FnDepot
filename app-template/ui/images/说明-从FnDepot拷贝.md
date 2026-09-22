桌面图标占位目录

真实图标从 FnDepot 现役应用拷（flown NAS 惯例 64px/256px）：
  cp /vol2/1000/workspace/FnDepot/<app>/src/fnos-native/ui/images/icon-64.png \
     /vol2/1000/workspace/FnDepot/<app>/src/fnos-native/ui/images/icon-256.png \
     到此目录 ui/images/

命名规范（官方横线式）：
  icon-64.png / icon-256.png
ui/config 用 {0} 占位符（→64/256）自动匹配。
商店图标 = fpk 根 ICON.PNG / ICON_256.PNG（另行放入模板根目录）。