{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.anipler;
  package = cfg.package;
in
{
  options.services.anipler = {
    enable = lib.mkEnableOption "Anipler service";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix { };
      description = "The Anipler package to use for the service.";
    };

    listenAddr = lib.mkOption {
      type = lib.types.str;
      default = "127.0.0.1";
    };

    port = lib.mkOption {
      type = lib.types.int;
      default = 8080;
    };

    workingDirectory = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/anipler";
      description = "Working directory for Anipler service.";
    };

    env = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Environment variables for Anipler service.";
    };

    envFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Path to a file containing environment variables for Anipler service.";
    };

    user = lib.mkOption {
      type = lib.types.str;
      default = "anipler";
      description = "User to run Anipler service as.";
    };

    group = lib.mkOption {
      type = lib.types.str;
      default = "anipler";
      description = "Group to run Anipler service as.";
    };
  };

  config = lib.mkIf cfg.enable {
    users = {
      groups.${cfg.group} = { };
      users.${cfg.user} = {
        inherit (cfg) group;
        isSystemUser = true;
        description = "Anipler service user";
        home = cfg.workingDirectory;
        createHome = true;
        shell = pkgs.bashInteractive;
        packages = [ pkgs.rsync ];
      };
    };

    systemd.services.anipler = {
      description = "Anipler Service";
      after = [ "network.target" ];
      wants = [ "network.target" ];
      serviceConfig = {
        User = cfg.user;
        Group = cfg.group;
        EnvironmentFile = lib.optional (cfg.envFile != null) cfg.envFile;
        ExecStart = lib.getExe' package "anipler-daemon";
        Restart = "on-failure";
        RestartSec = "60s";
      };
      wantedBy = [ "multi-user.target" ];

      environment = cfg.env // {
        ANIPLER_API_ADDR = "${cfg.listenAddr}:${toString cfg.port}";
        ANIPLER_STORAGE_PATH = cfg.workingDirectory;
      };
    };
  };
}
