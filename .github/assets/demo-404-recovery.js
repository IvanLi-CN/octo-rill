(function () {
  var marker = "/demo/";
  var href = window.location.href;
  var url = new URL(href);
  if (url.searchParams.has("d_restore")) {
    return;
  }
  var markerIndex = url.pathname.indexOf(marker);
  if (markerIndex === -1) {
    return;
  }

  var demoRoot = url.pathname.slice(0, markerIndex + marker.length);
  if (url.pathname === demoRoot || url.pathname === demoRoot.slice(0, -1)) {
    return;
  }

  var restore = encodeURIComponent(
    url.pathname + url.search + url.hash,
  );
  var target = new URL(demoRoot, window.location.origin);
  target.searchParams.set("d_restore", restore);
  window.location.replace(target.toString());
})();
