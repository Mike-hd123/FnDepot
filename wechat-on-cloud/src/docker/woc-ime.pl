# perl -0777 -pe 补丁脚本（被 woc-www-patch.sh 引用）。
# 对 dist/*.bundle.js 里 noVNC 键盘 IME 逻辑做两处替换，全程字面匹配（\Q..\E）。
#
# 背景：noVNC 原实现靠"隐藏 textarea 差分→逐字符重发 keysym"还原 IME 输入，会在合成过程中
# 把中间拼音也发给远端、且永不 reset 导致累积+退格风暴 → 大量丢字 / 卡住 / 跨浏览器不稳。
#
# 改法：彻底不靠 textarea 差分或 VNC keysym 还原中文。
#   - 合成进行中(input 事件)：只同步 _lastKeyboardInput、不发送（避免中间拼音泄漏 / 丢字）。
#   - 提交时(compositionend)：只同步 _lastKeyboardInput 并返回，不再逐字发 keysym。
#     成品文本由面板前端捕获后通过 xclip/xdotool 粘贴进远端窗口，绕开 KasmVNC XKB
#     keysym 容量限制，也避免和粘贴路径重复上屏。
#   - 若个别浏览器在 compositionend 后还补发一次"提交 input"：此时 isComposing/_imeHold 均为假，
#     落到非 IME 差分分支，但 newValue 与刚同步的 _lastKeyboardInput 相等 → 差分为空 → 不重复发送。

# (A) _handleCompositionEnd：提交时只同步 _lastKeyboardInput，文本由面板粘贴路径负责。
s~\Q      if (this._enableIME) {
        this._imeInProgress = false;
      }

      if (isChromiumBased()) {
        this._imeHold = false;
      }\E~      if (this._enableIME) { // WOC-IME
        this._imeInProgress = false;
        this._imeHold = false;
        this._lastKeyboardInput = e.target.value;
        return;
      }

      if (isChromiumBased()) {
        this._imeHold = false;
      }~;

# (B) _handleInput 顶部守卫：合成进行中只同步值、不发送；提交已在 compositionend 完成。
s~\Q      if (this._enableIME && this._imeHold) {
        Debug("IME input change, sending differential");\E~      if (this._enableIME && (e.isComposing || this._imeHold || this._imeInProgress)) { // WOC-IME
        this._lastKeyboardInput = e.target.value;
        return;
      }

      if (this._enableIME && this._imeHold) {
        Debug("IME input change, sending differential");~;
