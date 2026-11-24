const appearanceChoices = ['system', 'light', 'dark'];
const deviceAppearance = matchMedia('(prefers-color-scheme: dark)');
const appearanceCookie = 'rca-appearance=';
const savedAppearance = document.cookie.split('; ')
  .find(cookie => cookie.startsWith(appearanceCookie))?.slice(appearanceCookie.length);
let appearance = appearanceChoices.includes(savedAppearance) ? savedAppearance : 'system';

function applyAppearance() {
  document.documentElement.dataset.theme = appearance === 'system'
    ? (deviceAppearance.matches ? 'dark' : 'light')
    : appearance;
}

applyAppearance();
deviceAppearance.addEventListener('change', applyAppearance);

document.addEventListener('DOMContentLoaded', () => {
  const picker = document.getElementById('appearance');
  picker.value = appearance;
  picker.parentElement.hidden = false;
  picker.addEventListener('change', () => {
    appearance = picker.value;
    document.cookie = `${appearanceCookie}${appearance}; Max-Age=31536000; Path=/; SameSite=Lax`;
    applyAppearance();
  });
});
