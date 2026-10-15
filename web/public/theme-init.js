// Applies this browser's saved theme before the app loads, avoiding a flash
// of the wrong theme. (A separate file because the CSP forbids inline scripts.)
(function () {
  try {
    var t = localStorage.getItem('nodestarr.theme');
    if (t === 'system') t = matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark';
    if (t === 'light' || t === 'dark') document.documentElement.dataset.theme = t;
  } catch (e) {
    /* storage unavailable: keep the default */
  }
})();
