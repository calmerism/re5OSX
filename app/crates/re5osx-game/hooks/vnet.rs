// Hooks for the runtime's `vnet.rs` (see ../hooks.rs).
&[
    // The engine's sockets on the lobby's virtual LAN, and the network's
    // update around them.
    ("profiler_push", "rage::netFunctionProfiler::PushProfiler(char const*, bool)"),
    ("create_socket", "rage::netHardware::CreateSocket(rage::netSocket*, unsigned short, rage::netProtocol, rage::netSocketBlockingType, rage::netAddressFamily, unsigned int, unsigned int)"),
    ("socket_bind", "rage::netSocket::NativeBind()"),
    ("socket_shutdown", "rage::netSocket::NativeShutdown()"),
    ("socket_send", "rage::netSocket::Send(rage::netSocketAddress const&, void const*, unsigned int, rage::netSocketError*)"),
    ("socket_receive", "rage::netSocket::Receive(rage::netSocketAddress*, void*, int, rage::netSocketError*)"),
    ("socket_select", "rage::netSocket::Select(int, fd_set*, fd_set*, fd_set*, timeval*)"),
    ("local_ip_address", "rage::netHardware::GetLocalIpAddress(rage::netIpAddress*)"),
    ("public_ip_address", "rage::netHardware::GetPublicIpAddress(rage::netIpAddress*)"),
]
