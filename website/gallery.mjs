const choices = [...document.querySelectorAll('.capture-choice')];
const image = document.querySelector('#capture-image');
const title = document.querySelector('#capture-title');
const opener = document.querySelector('.capture-open');
const dialog = document.querySelector('.capture-dialog');
const viewer = document.querySelector('#viewer-image');
let selected = 0;

function select(index) {
  selected = (index + choices.length) % choices.length;
  const choice = choices[selected];
  choices.forEach((item) => {
    if (item === choice) item.setAttribute('aria-current', 'true');
    else item.removeAttribute('aria-current');
  });
  image.src = choice.href;
  image.alt = choice.dataset.alt;
  title.textContent = choice.dataset.title;
  opener.href = choice.href;
  if (dialog.open) showImage();
}

function showImage() {
  viewer.src = image.src;
  viewer.alt = image.alt;
  document.querySelector('#viewer-title').textContent = title.textContent;
}

choices.forEach((choice, index) => choice.addEventListener('click', (event) => {
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  select(index);
}));

opener.addEventListener('click', (event) => {
  if (event.metaKey || event.ctrlKey || event.shiftKey || event.altKey) return;
  event.preventDefault();
  showImage();
  dialog.showModal();
});
document.querySelector('.viewer-close').addEventListener('click', () => dialog.close());
document.querySelector('.viewer-prev').addEventListener('click', () => select(selected - 1));
document.querySelector('.viewer-next').addEventListener('click', () => select(selected + 1));
dialog.addEventListener('keydown', (event) => {
  if (!['ArrowLeft', 'ArrowRight'].includes(event.key)) return;
  event.preventDefault();
  select(selected + (event.key === 'ArrowRight' ? 1 : -1));
});
dialog.addEventListener('click', (event) => {
  if (event.target !== dialog) return;
  const bounds = dialog.getBoundingClientRect();
  if (event.clientX < bounds.left || event.clientX > bounds.right || event.clientY < bounds.top || event.clientY > bounds.bottom) dialog.close();
});
