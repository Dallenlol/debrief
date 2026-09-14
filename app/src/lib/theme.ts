/** Follow the OS colour scheme by toggling the `dark` class on <html>. */
export function installTheme() {
  const mq = window.matchMedia("(prefers-color-scheme: dark)");
  const apply = () => document.documentElement.classList.toggle("dark", mq.matches);
  apply();
  mq.addEventListener("change", apply);
}
