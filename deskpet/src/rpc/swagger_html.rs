//! The single HTML page served at `/`. Loads swagger-ui-dist from a CDN
//! and points it at our `/openapi.json`. Zero local bundling — the page
//! is a static string in the binary, ~3 KB.

/// Swagger UI HTML, served at `/`. Pulls swagger-ui-dist from jsDelivr
/// (CDN-hosted). For an air-gapped build, swap the CDN URLs for
/// locally-bundled swagger-ui assets.
pub const SWAGGER_HTML: &str = r##"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <title>deskpet RPC API</title>
  <link rel="stylesheet" href="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui.css">
  <style>
    body { margin: 0; padding: 0; }
    .topbar { display: none; }
  </style>
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://cdn.jsdelivr.net/npm/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.onload = () => {
      window.ui = SwaggerUIBundle({
        url: "/openapi.json",
        dom_id: "#swagger-ui",
        deepLinking: true,
        presets: [SwaggerUIBundle.presets.apis],
        layout: "BaseLayout"
      });
    };
  </script>
</body>
</html>
"##;