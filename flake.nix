{
  description = "A privacy-focused Twitter/X frontend written in Rust";

  outputs =
    { self, ... }@args:
    let
      inputs = (import ./.tack) { overrides = args.tackOverrides or { }; };
      inherit (inputs) nixpkgs fenix;
      inherit (nixpkgs) lib;
      forAllSystems = lib.genAttrs (lib.systems.doubles.linux ++ lib.systems.doubles.darwin);
      pkgsFor = system: nixpkgs.legacyPackages.${system} or (import nixpkgs { inherit system; });

      hasFenix = system: fenix.packages ? ${system};
      rustPlatformFor =
        system:
        let
          pkgs = pkgsFor system;
          fenixPkgs = fenix.packages.${system}.latest;
        in
        if hasFenix system then
          pkgs.makeRustPlatform { inherit (fenixPkgs) cargo rustc; }
        else
          pkgs.rustPlatform;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
        in
        {
          default = pkgs.callPackage ./nix/package.nix {
            rustPlatform = rustPlatformFor system;
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = pkgsFor system;
          toolchain =
            if hasFenix system then
              [ fenix.packages.${system}.latest.toolchain ]
            else
              (with pkgs; [
                cargo
                rustc
                rustfmt
              ]);
        in
        {
          default = pkgs.mkShell {
            buildInputs = toolchain ++ [
              pkgs.rust-analyzer
              pkgs.pkg-config
              pkgs.mold
              pkgs.clang
              pkgs.ffmpeg-headless
            ];
          };
        }
      );

      nixosModules.default =
        {
          config,
          lib,
          pkgs,
          ...
        }:
        let
          cfg = config.services.teapot;
          configFile = pkgs.writeText "teapot.toml" ''
            ${lib.generators.toINI
              {
                mkKeyValue = lib.generators.mkKeyValueDefault {
                  mkValueString =
                    v:
                    if lib.isString v then
                      "\"" + (lib.escape [ "\"" ] (toString v)) + "\""
                    else
                      lib.generators.mkValueStringDefault { } v;
                } " = ";
              }
              (
                lib.recursiveUpdate {
                  inherit (cfg) cache preferences gifTranscoding;
                  config = cfg.config // {
                    hmacKey = "@hmac@";
                    kagiToken = "@kagi@";
                  };
                  server = lib.filterAttrs (_: v: v != null) cfg.server // {
                    staticDir = "${cfg.package}/share/teapot/public";
                  };
                } cfg.settings
              )
            }
          '';
          preStart = pkgs.writers.writePython3 "teapot-prestart" { } ''
            import os
            import secrets

            state_dir = os.environ.get("STATE_DIRECTORY")
            creds_dir = os.environ.get("CREDENTIALS_DIRECTORY", "")
            if not os.path.isfile(f"{state_dir}/hmac"):
                hmac = secrets.token_hex(32)
                with open(f"{state_dir}/hmac", "w") as f:
                    f.write(hmac)
            else:
                with open(f"{state_dir}/hmac", "r") as f:
                    hmac = f.read()

            kagi = ""
            kagi_cred = os.path.join(creds_dir, "kagiToken") if creds_dir else ""
            if kagi_cred and os.path.isfile(kagi_cred):
                with open(kagi_cred, "r") as f:
                    kagi = f.read().strip()

            configFile = "${configFile}"
            with open(configFile, "r") as f_in:
                content = f_in.read()
                content = content.replace("@hmac@", hmac)
                content = content.replace("@kagi@", kagi)
                with open(f"{state_dir}/teapot.toml", "w") as f_out:
                    f_out.write(content)
          '';
        in
        {
          options.services.teapot = {
            enable = lib.mkEnableOption "teapot, a privacy-focused Twitter/X frontend";

            package = lib.mkOption {
              type = lib.types.package;
              # built from the caller's pkgs, including cross stdenvs, so any
              # host platform works without the flake having to enumerate it
              default = pkgs.callPackage ./nix/package.nix { };
              defaultText = lib.literalExpression "pkgs.callPackage ./nix/package.nix { }";
              description = "The teapot package to use.";
            };

            server = {
              hostname = lib.mkOption {
                type = lib.types.str;
                default = "localhost";
                example = "teapot.example.com";
                description = "Hostname of the instance.";
              };
              title = lib.mkOption {
                type = lib.types.str;
                default = "teapot";
                description = "Title of the instance.";
              };
              address = lib.mkOption {
                type = lib.types.str;
                default = "0.0.0.0";
                example = "127.0.0.1";
                description = "The address to listen on.";
              };
              port = lib.mkOption {
                type = lib.types.port;
                default = 8080;
                description = "The port to listen on.";
              };
              publicPort = lib.mkOption {
                type = lib.types.nullOr lib.types.port;
                default = null;
                description = "Port used for generating public URLs. Defaults to the listen port.";
              };
              https = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = "Set secure attribute on cookies. Enable when using HTTPS.";
              };
              httpMaxConnections = lib.mkOption {
                type = lib.types.int;
                default = 100;
                description = "Maximum number of HTTP connections.";
              };
            };

            cache = {
              listMinutes = lib.mkOption {
                type = lib.types.int;
                default = 240;
                description = "How long to cache list info (minutes).";
              };
              rssMinutes = lib.mkOption {
                type = lib.types.int;
                default = 10;
                description = "How long to cache RSS queries (minutes).";
              };
            };

            config = {
              base64Media = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = "Use base64 encoding for proxied media URLs.";
              };
              enableRSS = lib.mkEnableOption "RSS feeds" // {
                default = true;
              };
              enableDebug = lib.mkEnableOption "request logs and debug endpoints";
              proxy = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "URL to a HTTP/HTTPS proxy.";
              };
              proxyAuth = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "Credentials for proxy.";
              };
              apiProxy = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "API proxy host for requests.";
              };
              disableTid = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = "Disable TID for cookie-based auth.";
              };
              maxConcurrentReqs = lib.mkOption {
                type = lib.types.int;
                default = 2;
                description = "Max concurrent requests per session.";
              };
              paidEmoji = lib.mkOption {
                type = lib.types.str;
                default = "🤝";
                description = "Emoji for paid promotion disclosure labels.";
              };
              aiEmoji = lib.mkOption {
                type = lib.types.str;
                default = "🤖";
                description = "Emoji for AI-generated content disclosure labels.";
              };
            };

            gifTranscoding = {
              mode = lib.mkOption {
                type = lib.types.enum [
                  "off"
                  "local"
                  "external"
                ];
                default = "local";
                description = "GIF transcoding mode. 'local' requires ffmpeg, 'external' rewrites URLs to an external domain.";
              };
              cacheDir = lib.mkOption {
                type = lib.types.str;
                default = "/var/lib/teapot/cache/gif";
                description = "Cache directory for transcoded GIFs (local mode).";
              };
              cacheMaxMb = lib.mkOption {
                type = lib.types.int;
                default = 512;
                description = "Maximum cache size in MB (local mode).";
              };
              externalDomain = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "External transcoding domain (external mode).";
              };
            };

            preferences = {
              theme = lib.mkOption {
                type = lib.types.str;
                default = "teapot";
                description = "Instance theme.";
              };
              replaceTwitter = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "Replace Twitter links with links to this instance.";
              };
              replaceYouTube = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "Replace YouTube links with this instance.";
              };
              replaceReddit = lib.mkOption {
                type = lib.types.str;
                default = "";
                description = "Replace Reddit links with this instance.";
              };
              proxyVideos = lib.mkOption {
                type = lib.types.bool;
                default = true;
                description = "Proxy video streaming through the server.";
              };
              infiniteScroll = lib.mkOption {
                type = lib.types.bool;
                default = false;
                description = "Infinite scrolling (requires JavaScript).";
              };
            };

            settings = lib.mkOption {
              type = lib.types.attrs;
              default = { };
              description = "Additional settings to override module-generated config.";
            };

            sessionsFile = lib.mkOption {
              type = lib.types.path;
              default = "/var/lib/teapot/sessions.jsonl";
              description = ''
                Path to the session tokens file (JSONL format).

                Each line: {"oauth_token":"...","oauth_token_secret":"..."}
              '';
            };

            kagiTokenFile = lib.mkOption {
              type = lib.types.nullOr lib.types.path;
              default = null;
              description = ''
                Path to a file containing a Kagi session token for server-side
                translation. When set, non-English tweets are translated via
                Kagi Translate instead of Twitter's Strato API.

                Get your token from kagi.com/settings → Session Link.
              '';
            };

            openFirewall = lib.mkOption {
              type = lib.types.bool;
              default = false;
              description = "Open ports in the firewall for the web interface.";
            };
          };

          config = lib.mkIf cfg.enable {
            systemd.services.teapot = {
              description = "teapot (privacy-focused Twitter/X frontend)";
              wantedBy = [ "multi-user.target" ];
              wants = [ "network-online.target" ];
              after = [ "network-online.target" ];
              path = lib.mkIf (cfg.gifTranscoding.mode == "local") [ pkgs.ffmpeg-headless ];
              serviceConfig = {
                DynamicUser = true;
                LoadCredential = [
                  "sessionsFile:${cfg.sessionsFile}"
                ]
                ++ lib.optional (cfg.kagiTokenFile != null) "kagiToken:${cfg.kagiTokenFile}";
                StateDirectory = "teapot";
                Environment = [
                  "TEAPOT_CONF_FILE=/var/lib/teapot/teapot.toml"
                  "TEAPOT_SESSIONS_FILE=%d/sessionsFile"
                ];
                WorkingDirectory = "${cfg.package}/share/teapot";
                ExecStart = "${cfg.package}/bin/teapot";
                ExecStartPre = "${preStart}";
                AmbientCapabilities = lib.mkIf (cfg.server.port < 1024) [ "CAP_NET_BIND_SERVICE" ];
                Restart = "on-failure";
                RestartSec = "5s";
                # Hardening
                CapabilityBoundingSet = if (cfg.server.port < 1024) then [ "CAP_NET_BIND_SERVICE" ] else [ "" ];
                DeviceAllow = [ "" ];
                LockPersonality = true;
                MemoryDenyWriteExecute = true;
                PrivateDevices = true;
                PrivateUsers = cfg.server.port >= 1024;
                ProcSubset = "pid";
                ProtectClock = true;
                ProtectControlGroups = true;
                ProtectHome = true;
                ProtectHostname = true;
                ProtectKernelLogs = true;
                ProtectKernelModules = true;
                ProtectKernelTunables = true;
                ProtectProc = "invisible";
                RestrictAddressFamilies = [
                  "AF_INET"
                  "AF_INET6"
                ];
                RestrictNamespaces = true;
                RestrictRealtime = true;
                RestrictSUIDSGID = true;
                SystemCallArchitectures = "native";
                SystemCallFilter = [
                  "@system-service"
                  "~@privileged"
                  "~@resources"
                ];
                UMask = "0077";
              };
            };

            networking.firewall = lib.mkIf cfg.openFirewall {
              allowedTCPPorts = [ cfg.server.port ];
            };
          };
        };
    };
}
