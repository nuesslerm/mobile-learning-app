import { CM6_BUNDLE_JS } from './cm6Bundle';

/**
 * Static HTML template for the CodeMirror 6 WebView.
 * All configuration is delivered via postMessage after the WebView signals 'ready'.
 */
export function buildEditorHtml(options: { debug?: boolean; profileInput?: boolean } = {}): string {
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
${options.debug ? '<script>window.__EDITOR_DEBUG__ = true;</script>\n' : ''}<script>window.__cm6T0 = performance.now();</script>
${options.profileInput ? '<script>window.__EDITOR_PROFILE_INPUT__ = true;</script>\n' : ''}<script>${CM6_BUNDLE_JS}</script>
<script>window.__cm6EvalMs = Math.round(performance.now() - window.__cm6T0);</script>
<script>
  document.addEventListener('DOMContentLoaded', function () {
    if (window.ReactNativeWebView) {
      window.ReactNativeWebView.postMessage(JSON.stringify({ type: 'ready', evalMs: window.__cm6EvalMs }));
    }
  });
</script>
</body>
</html>`;
}
