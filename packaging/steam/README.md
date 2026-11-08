# Steam depot upload

Needs the Steamworks app (free listing) and its depot IDs. Fill `APP_ID`, `DEPOT_WINDOWS`, `DEPOT_LINUX` in
`app_build.vdf`, unpack the release archives into `content/windows` and `content/linux`, then:

```
steamcmd +login <builder-account> +run_app_build "%CD%\packaging\steam\app_build.vdf" +quit
```

Use a dedicated builder account with Steam Guard; never commit credentials. Builds land on a beta branch;
set them live in the Steamworks partner site after testing.
