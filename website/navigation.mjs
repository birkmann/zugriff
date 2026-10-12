const root = document.documentElement;
const header = document.querySelector('.header');
const menu = document.querySelector('#main-navigation');
const toggle = document.querySelector('.menu-toggle');
const backdrop = document.querySelector('.nav-backdrop');
const mobile = matchMedia('(max-width: 700px)');
const background = [...document.querySelectorAll('main, footer, .skip, .header > .brand, .header > .nav-download')];
const links = [...menu.querySelectorAll('a')];
let open = false;
let savedScroll = 0;

function setOpen(next, returnFocus = true) {
  next = next && mobile.matches;
  if (open === next) return;
  if (next) {
    savedScroll = window.scrollY;
    root.classList.add('nav-animated');
  }
  open = next;
  root.classList.toggle('nav-open', open);
  toggle.setAttribute('aria-expanded', String(open));
  toggle.setAttribute('aria-label', open ? 'Close navigation menu' : 'Open navigation menu');
  menu.inert = mobile.matches && !open;
  background.forEach(element => { element.inert = open; });
  if (open) {
    document.body.style.top = `-${savedScroll}px`;
    root.classList.add('nav-locked');
    requestAnimationFrame(() => requestAnimationFrame(() => {
      if (open) links[0].focus({ preventScroll: true });
    }));
  } else {
    root.classList.remove('nav-locked');
    document.body.style.top = '';
    window.scrollTo({ top: savedScroll, behavior: 'instant' });
    if (returnFocus && mobile.matches) toggle.focus({ preventScroll: true });
  }
}

function syncViewport() {
  const hadMenuFocus = menu.contains(document.activeElement) || document.activeElement === toggle;
  setOpen(false, false);
  menu.inert = mobile.matches;
  if (hadMenuFocus) (mobile.matches ? toggle : links[0]).focus({ preventScroll: true });
}

toggle.hidden = false;
root.classList.add('nav-ready');
syncViewport();
toggle.addEventListener('click', () => setOpen(!open));
backdrop.addEventListener('click', () => setOpen(false));
mobile.addEventListener('change', syncViewport);
links.forEach(link => link.addEventListener('click', event => {
  if (!open || event.ctrlKey || event.metaKey || event.shiftKey || event.altKey) return;
  setOpen(false, false);
  // Keep normal anchor navigation and give keyboard users the destination.
  const target = document.querySelector(link.hash);
  if (target) {
    target.setAttribute('tabindex', '-1');
    target.focus({ preventScroll: true });
    target.addEventListener('blur', () => target.removeAttribute('tabindex'), { once: true });
  }
}));
document.addEventListener('keydown', event => {
  if (!open) return;
  if (event.key === 'Escape') {
    event.preventDefault();
    setOpen(false);
  } else if (event.key === 'Tab') {
    const items = [toggle, ...links];
    const index = items.indexOf(document.activeElement);
    event.preventDefault();
    items[(index + (event.shiftKey ? -1 : 1) + items.length) % items.length].focus();
  }
});
const updateHeader = () => header.classList.toggle('is-scrolled', window.scrollY > 12);
window.addEventListener('scroll', updateHeader, { passive: true });
updateHeader();
