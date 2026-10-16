use std::sync::Arc;
use tokio::sync::Mutex;
use webrtc::{api::setting_engine::SettingEngine, ice::{mdns::MulticastDnsMode, network_type::NetworkType},
    util::vnet::{net::{Net, NetConfig}, router::{Router, RouterConfig}}};

pub(crate) async fn virtual_network() -> (SettingEngine, SettingEngine, Arc<Mutex<Router>>) {
    // Each pair owns its router and address space; no host interfaces or sockets are used.
    let router = Arc::new(Mutex::new(Router::new(RouterConfig {
        cidr: "192.0.2.0/24".into(), ..Default::default()
    }).unwrap()));
    let mut settings = Vec::new();
    for ip in ["192.0.2.1", "192.0.2.2"] {
        let net = Arc::new(Net::new(Some(NetConfig { static_ips: vec![ip.into()], ..Default::default() })));
        let nic = net.get_nic().unwrap();
        router.lock().await.add_net(nic.clone()).await.unwrap();
        nic.lock().await.set_router(router.clone()).await.unwrap();
        let mut setting = SettingEngine::default();
        setting.set_vnet(Some(net));
        setting.set_network_types(vec![NetworkType::Udp4]);
        setting.set_ice_multicast_dns_mode(MulticastDnsMode::Disabled);
        settings.push(setting);
    }
    router.lock().await.start().await.unwrap();
    let right = settings.pop().unwrap();
    (settings.pop().unwrap(), right, router)
}
