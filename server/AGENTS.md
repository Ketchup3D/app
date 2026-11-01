# Ketchup Server

This directory is the Ketchup Server project. When the user says “dans Ketchup Server”, work in this directory: `/home/dcoulombe/dev/ketchup/server`.

- The server application must be implemented in PHP. Do not replace it with a Rust, Node.js, Python, or other server implementation.
- Current scope: a working foundation for Google login, authenticated sessions, and logout. Shared project links and concurrent editing are future work, not part of the current foundation.
- Keep server changes inside this directory unless the user requests integration with the KetchUp client.
- Never commit credentials. Provide example configuration and document the required Google Cloud OAuth client setup.
- Do not bypass authentication or fabricate a successful Google login when credentials are unavailable. Test the flow with an injected provider transport and clearly state what requires real Google credentials.
