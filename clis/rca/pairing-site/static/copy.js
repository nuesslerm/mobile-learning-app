const clipboard = navigator.clipboard;

if (clipboard) {
  const status = document.getElementById('copy-status');
  for (const button of document.querySelectorAll('[data-copy]')) {
    const value = document.getElementById(button.dataset.copy);
    const label = button.getAttribute('aria-label').slice('Copy '.length);
    button.hidden = false;
    button.addEventListener('click', () => {
      clipboard.writeText(value.textContent).then(
        () => { status.textContent = `Copied ${label}.`; },
        () => { status.textContent = 'Touch and hold the value to copy it manually.'; },
      );
    });
  }
}
