(async () => {
  try {
    await window.document.fonts.ready;
  } catch (e) {}
  return JSON.stringify({
    ready: window.document.readyState,
    inflight: window.__visualInflight || 0,
    resources: window.performance.getEntriesByType('resource').length,
    errors: window.__visualErrors || [],
  });
})()
