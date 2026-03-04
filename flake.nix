{
  description = "teapot - A privacy-focused Twitter/X frontend written in Rust";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixpkgs-unstable";
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      self,
      nixpkgs,
      fenix,
    }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          fenixPkgs = fenix.packages.${system}.latest;
          rustPlatform = pkgs.makeRustPlatform {
            cargo = fenixPkgs.cargo;
            rustc = fenixPkgs.rustc;
          };
        in
        {
          default = rustPlatform.buildRustPackage {
            pname = "teapot";
            version = "0.1.0";

            src = pkgs.lib.fileset.toSource {
              root = ./.;
              fileset = pkgs.lib.fileset.unions [
                ./Cargo.toml
                ./Cargo.lock
                ./src
                ./public
                ./config
              ];
            };

            cargoLock.lockFile = ./Cargo.lock;

            nativeBuildInputs = [
              pkgs.pkg-config
            ];

            doCheck = false;
            stripAllList = [ "bin" ];

            postInstall = ''
              mkdir -p $out/share/teapot
              cp -r public $out/share/teapot/
              cp -r config $out/share/teapot/
            '';

            meta = with pkgs.lib; {
              description = "A privacy-focused Twitter/X frontend written in Rust";
              homepage = "https://github.com/amaanq/teapot";
              license = licenses.agpl3Only;
              mainProgram = "teapot";
            };
          };
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          toolchain = fenix.packages.${system}.latest.toolchain;
        in
        {
          default = pkgs.mkShell {
            buildInputs = [
              toolchain
              pkgs.rust-analyzer
              pkgs.pkg-config
            ];

            shellHook = ''
              echo "teapot development shell"
              echo ""
              echo "Quick start:"
              echo "  1. cp config/teapot.example.toml config/teapot.toml"
              echo "  2. Create sessions.jsonl with your tokens"
              echo "  3. cargo run"
            '';
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
                  inherit (cfg) cache preferences;
                  config = cfg.config // {
                    hmacKey = "@hmac@";
                  };
                  server = cfg.server // {
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
            if not os.path.isfile(f"{state_dir}/hmac"):
                hmac = secrets.token_hex(32)
                with open(f"{state_dir}/hmac", "w") as f:
                    f.write(hmac)
            else:
                with open(f"{state_dir}/hmac", "r") as f:
                    hmac = f.read()

            configFile = "${configFile}"
            with open(configFile, "r") as f_in:
                with open(f"{state_dir}/teapot.toml", "w") as f_out:
                    f_out.write(f_in.read().replace("@hmac@", hmac))
          '';
        in
        {
          options.services.teapot = {
            enable = lib.mkEnableOption "teapot, a privacy-focused Twitter/X frontend";

            package = lib.mkPackageOption self.packages.${pkgs.system} "default" { };

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
              serviceConfig = {
                DynamicUser = true;
                LoadCredential = "sessionsFile:${cfg.sessionsFile}";
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
