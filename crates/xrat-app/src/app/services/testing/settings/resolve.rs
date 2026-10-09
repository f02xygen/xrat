use super::*;
use crate::app::geoip_backend::build_lookup_chain;
use crate::app::paths::mmdb;

pub(crate) fn resolve_test_settings(
    request: &TestRunRequest,
    app_config: &AppConfig,
    runtime_paths: &RuntimePaths,
) -> crate::app::Result<ResolvedTestSettings> {
    if app_config.runtime.engine == "sing-box" {
        xrat_engines::singbox::ensure_supported_binary(&runtime_paths.sing_box_path)?;
    }
    let concurrency = request
        .concurrency
        .unwrap_or(app_config.testing.concurrency);
    if concurrency < 0 {
        return Err(AppError::InvalidArgument(
            "test concurrency must be 0 or greater".to_string(),
        ));
    }
    validate_test_stage_order(&app_config.testing.order)?;
    let real_delay = &app_config.testing.real_delay;
    let accepted_http_statuses = if real_delay.accepted_status_codes.is_none()
        && real_delay.accepted_status_ranges.is_none()
    {
        AcceptedHttpStatuses::default()
    } else {
        AcceptedHttpStatuses::new(
            real_delay.accepted_status_codes.clone().unwrap_or_default(),
            real_delay
                .accepted_status_ranges
                .as_deref()
                .unwrap_or_default()
                .iter()
                .map(|range| range.bounds())
                .collect(),
        )
        .map_err(|error| {
            AppError::InvalidArgument(format!(
                "invalid [testing.real_delay] status acceptance: {error}"
            ))
        })?
    };
    let geoip_country_path = mmdb::mmdb_path_for(
        runtime_paths,
        app_config,
        &app_config.testing.geoip.country_path,
        "GeoLite2-Country.mmdb",
    );
    let geoip_city_path = mmdb::mmdb_path_for(
        runtime_paths,
        app_config,
        &app_config.testing.geoip.city_path,
        "GeoLite2-City.mmdb",
    );
    let geoip_asn_path = mmdb::mmdb_path_for(
        runtime_paths,
        app_config,
        &app_config.testing.geoip.asn_path,
        "GeoLite2-ASN.mmdb",
    );
    let geoip_lookup = build_lookup_chain(app_config, runtime_paths)?;
    let xray_binary_path = resolve_engine_binary_path(app_config, runtime_paths);
    let probe_engine = if app_config.runtime.engine == "sing-box" {
        xrat_prober::ProbeEngineKind::Singbox
    } else {
        xrat_prober::ProbeEngineKind::Xray
    };
    let mut gen_options =
        crate::app::services::runtime_tuning::build_xray_gen_options(&app_config.runtime);
    gen_options.compatibility = crate::app::services::runtime_tuning::detect_xray_compatibility(
        app_config.runtime.xray_compatibility,
        &xray_binary_path,
    );
    crate::app::services::runtime_tuning::dns_validation::validate(
        &app_config.dns,
        &app_config.runtime.engine,
        app_config.runtime.tun.enabled,
    )
    .map_err(AppError::InvalidArgument)?;
    if app_config.runtime.engine == "sing-box" {
        let mut probe_dns = app_config.dns.clone();
        probe_dns.fakeip.enabled = false;
        gen_options.singbox_dns =
            crate::app::services::runtime_tuning::build_singbox_dns_options(&probe_dns)?;
        gen_options.bootstrap_resolver = (!app_config.dns.resolvers.is_empty())
            .then(|| app_config.dns.bootstrap_resolver.clone());
    } else {
        crate::app::services::runtime_tuning::apply_xray_dns_options(
            &mut gen_options,
            &app_config.dns,
        )?;
    }

    Ok(ResolvedTestSettings {
        stage_order: app_config.testing.order.clone(),
        failure_policy: app_config.testing.failure_policy,
        real_delay_url: request
            .test_url
            .clone()
            .unwrap_or_else(|| app_config.testing.real_delay.url.clone()),
        accepted_http_statuses,
        follow_redirects: real_delay.follow_redirects,
        download_url: request
            .download_url
            .clone()
            .unwrap_or_else(|| app_config.testing.download.url.clone()),
        upload_url: request.upload_url.clone(),
        xray_binary_path,
        probe_engine,
        icmp_timeout: Duration::from_millis(
            request
                .icmp_timeout_ms
                .unwrap_or(app_config.testing.icmp.timeout),
        ),
        tcp_timeout: Duration::from_millis(
            request
                .tcp_timeout_ms
                .unwrap_or(app_config.testing.tcp.timeout),
        ),
        xray_startup_timeout: Duration::from_millis(defaults::DEFAULT_XRAY_STARTUP_TIMEOUT_MS),
        real_delay_timeout: Duration::from_millis(
            request
                .real_delay_timeout_ms
                .unwrap_or(app_config.testing.real_delay.timeout),
        ),
        download_timeout: Duration::from_millis(
            request
                .download_timeout_ms
                .unwrap_or(app_config.testing.download.timeout),
        ),
        upload_timeout: Duration::from_millis(
            request
                .upload_timeout_ms
                .unwrap_or(defaults::DEFAULT_UPLOAD_TIMEOUT_MS),
        ),
        upload_payload_bytes: defaults::DEFAULT_UPLOAD_PAYLOAD_BYTES,
        run_icmp: app_config.testing.icmp.enabled && !request.skip_icmp,
        run_tcp: app_config.testing.tcp.enabled && !request.skip_tcp,
        run_real_delay: app_config.testing.real_delay.enabled && !request.skip_real_delay,
        run_download: app_config.testing.download.enabled && !request.skip_download,
        run_upload: request.upload_url.is_some() && !request.skip_upload,
        concurrency,
        geoip_enabled: app_config.testing.geoip.enabled,
        geoip_country_path,
        geoip_city_path,
        geoip_asn_path,
        geoip_lookup,
        gen_options,
    })
}
