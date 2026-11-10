(() => {
  const limits = { maxErrors: 20, maxErrorChars: 600 };
  const page = window;
  const errors = [];
  page.__visualErrors = errors;
  page.__visualInflight = 0;
  const record = (message) => {
    if (errors.length < limits.maxErrors) errors.push(String(message).slice(0, limits.maxErrorChars));
  };
  const describe = (value) => (typeof value === 'object' && value !== null && 'stack' in value && typeof value.stack === 'string' ? value.stack : String(value));
  const consoleError = page.console.error;
  page.console.error = (...args) => {
    record(args.map(describe).join(' '));
    consoleError.apply(page.console, args);
  };
  page.addEventListener(
    'error',
    (event) => {
      const resource = [event.target && event.target.src, event.target && event.target.href].find(Boolean);
      if (event.target !== page && resource) record(`resource failed: ${resource}`);
      else record(`uncaught: ${(event.error && event.error.stack) || event.message}`);
    },
    true
  );
  page.addEventListener('unhandledrejection', (event) => {
    record(`unhandled rejection: ${describe(event.reason)}`);
  });
  const pageFetch = page.fetch;
  if (pageFetch) {
    page.fetch = (...args) => {
      page.__visualInflight += 1;
      return pageFetch.apply(page, args).finally(() => {
        page.__visualInflight -= 1;
      });
    };
  }
  const xhrSend = page.XMLHttpRequest.prototype.send;
  page.XMLHttpRequest.prototype.send = function (...args) {
    page.__visualInflight += 1;
    this.addEventListener('loadend', () => {
      page.__visualInflight -= 1;
    });
    xhrSend.apply(this, args);
  };
})();
