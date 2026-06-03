{
  config,
  lib,
  pkgs,
  ...
}:
let
  cfg = config.services.anipler;
  package = cfg.package;
  configFormat = pkgs.formats.toml { };
  configFile = configFormat.generate "anipler-daemon.toml" cfg.settings;
in
{
  options.services.anipler = {
    enable = lib.mkEnableOption "Anipler service";

    package = lib.mkOption {
      type = lib.types.package;
      description = "The Anipler package to use for the service.";
    };

    settings = lib.mkOption {
      type = configFormat.type;
      default = {
        storage_path = "/var/lib/anipler";
      };
      description = ''
        Anipler daemon configuration written to a TOML file.

        Secrets should usually be provided through systemd's EnvironmentFile
        with ANIPLER_* variables so that secrets are not stored in Nix store.
      '';
    };

    workingDirectory = lib.mkOption {
      type = lib.types.path;
      default = "/var/lib/anipler";
      description = "Working directory for Anipler service.";
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
        ExecStart = "${lib.getExe' package "anipler-daemon"} --config ${configFile}";
        Restart = "on-failure";
        RestartSec = "60s";
      };
      wantedBy = [ "multi-user.target" ];

      path = with pkgs; [
        openssh
        rsync
      ];

      environment = {
        "ANIPLER_STORAGE_PATH" = cfg.workingDirectory;
      };
    };
  };
}
