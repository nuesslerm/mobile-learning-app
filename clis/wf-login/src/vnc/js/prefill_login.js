(() => {
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const result = { email: false };

  const field = document.querySelector(
    'input[type="email"], input[autocomplete="username"], input[autocomplete="email"], input[name*="email" i], input[id*="email" i]'
  );
  if (field && visible(field)) {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      'value'
    ).set;
    setter.call(field, EMAIL_PLACEHOLDER);
    // React and friends listen for input/change, not for a raw value write.
    field.dispatchEvent(new Event('input', { bubbles: true }));
    field.dispatchEvent(new Event('change', { bubbles: true }));
    field.focus();
    result.email = true;
  }
  return result;
})();
