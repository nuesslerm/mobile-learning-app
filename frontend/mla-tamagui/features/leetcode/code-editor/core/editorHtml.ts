import { CM6_BUNDLE_JS } from './cm6Bundle';

/**
 * Static HTML template for the CodeMirror 6 WebView.
 * All configuration is delivered via postMessage after the WebView signals 'ready'.
 */
export function buildEditorHtml(): string {
  return `<!DOCTYPE html>
<html>
<head>
<meta name="viewport" content="width=device-width, initial-scale=1, maximum-scale=1, user-scalable=no">
<style>
  * { margin: 0; padding: 0; box-sizing: border-box; }
  html, body { height: 100%; overflow: hidden; }
  #editor { height: 100%; }
  .cm-editor { height: 100%; }
  .cm-editor .cm-scroller { overflow: auto; }
  @keyframes highlightFlash {
     0% { background-color: rgba(255, 235, 59, 0.6); }
    100% { background-color: transparent; }
  }
  .cm-highlight-flash {
    animation: highlightFlash 3000ms ease-out forwards;
  }
  .cm-diff-change {
    background-color: rgba(34, 197, 94, 0.25);
  }
</style>
</head>
<body>
<div id="editor"></div>
<!--
  iOS WKWebView fix: explicit JS blur() marks the contenteditable as "voluntarily
  resigned", preventing the keyboard from re-appearing on the next native tap.
  We intercept the 'blur' message before it reaches CM6 and instead briefly focus
  a hidden button. That causes a browser-native blur on CM6's contenteditable
  (which doesn't carry the "voluntarily resigned" restriction), then immediately
  releases that focus so no element is held — leaving the WebView in a clean
  state where the next user tap on the editor restores the keyboard normally.
-->
<button id="kb-blur-target" tabindex="-1" aria-hidden="true"
  style="position:absolute;opacity:0;width:0;height:0;pointer-events:none;border:none;background:none;"></button>
<script>
  window.addEventListener('error', function (event) {
    if (window.ReactNativeWebView) {
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'error', message: String(event.message) }));
    }
  });
  (function () {
    var _dispatch = window.dispatchEvent.bind(window);
    window.dispatchEvent = function (event) {
      if (event && event.type === 'message') {
        try {
          var msg = JSON.parse(event.data);
          if (msg && msg.type === 'blur') {
            var btn = document.getElementById('kb-blur-target');
            if (btn) {
              btn.focus();
              requestAnimationFrame(function () { btn.blur(); });
            }
            return true;
          }
        } catch (e) {}
      }
      return _dispatch(event);
    };
  })();
</script>
<script>${CM6_BUNDLE_JS}</script>
<script>
  document.addEventListener('DOMContentLoaded', function () {
    if (window.ReactNativeWebView) {
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'ready' }));
    }
  });
</script>
</body>
</html>`;
}
