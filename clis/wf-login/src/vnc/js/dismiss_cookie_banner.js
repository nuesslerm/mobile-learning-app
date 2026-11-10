(() => {
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    if (r.width <= 0 || r.height <= 0) return false;
    const cs = getComputedStyle(el);
    return cs.visibility !== 'hidden' && cs.display !== 'none';
  };
  const textOf = (el) => (el.textContent || '').trim();

  // Reject first, accept only as a fallback: a desktop that silently opts a human into tracking is making their choice
  // for them, so the only reason to accept is that the banner offers no way not to and leaving it up covers the form.
  // No trailing word boundary: textOf concatenates a control's accessible name with its visible text, and a consent
  // platform routinely sets both to near-duplicates ("RejectReject All Cookies"), which a boundary after "reject" misses.
  const REJECT =
    /^(reject|decline|refuse|deny|only necessary|necessary only|essentials? only|essential cookies only|use necessary|continue without accepting|manage.*reject)/i;
  const ACCEPT = /^(accept|allow all|agree|i agree|got it|ok\b)/i;

  const clickable = [
    ...document.querySelectorAll(
      'button, [role="button"], a, input[type="button"], input[type="submit"]'
    ),
  ];
  const vis = clickable.filter(visible);

  const rejected = vis.find((el) => REJECT.test(textOf(el)));
  if (rejected) {
    rejected.click();
    return {
      dismissed: true,
      action: 'reject',
      label: textOf(rejected).slice(0, 60),
    };
  }

  const accepted = vis.find((el) => ACCEPT.test(textOf(el)));
  if (accepted) {
    accepted.click();
    return {
      dismissed: true,
      action: 'accept',
      label: textOf(accepted).slice(0, 60),
    };
  }

  return { dismissed: false };
})();
