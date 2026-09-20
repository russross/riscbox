Riscbox WebSocket Ethernet protocol
===================================

The browser network adapter connects to an HTTP origin endpoint upgraded to a
WebSocket. Deployments choose the endpoint URL when constructing
`WebSocketNetwork`; it is not part of the guest configuration.

Wire format
-----------

Each binary WebSocket message is exactly one Ethernet frame. The first byte is
the first destination-MAC byte. Messages contain neither a VirtIO header nor an
Ethernet frame-check sequence. Message order is Ethernet frame order in each
direction.

Version one does not negotiate a WebSocket subprotocol and has no application
header or text messages. A text message is a protocol error and the client
closes the connection with status 1003. Standard WebSocket ping, pong, and close
frames provide transport keepalive and lifecycle behavior.

Limits and loss
---------------

Frames must contain between 1 and 65,535 bytes. The client closes the connection
after an invalid inbound message. Ethernet is lossy: the client may drop guest
frames while disconnected or after the WebSocket send buffer reaches 1 MiB,
and the VM may drop inbound frames when its bounded receive queues are full.

The client reports carrier up only while the WebSocket is open. Error, close,
and reconnect delay report carrier down. Reconnection begins after 250 ms,
doubles to a 10-second ceiling, and resets after a successful open.

Future origin service
---------------------

A production origin service must authenticate and authorize the upgrade,
isolate layer-2 clients, validate and rate-limit frames, bridge them to an
explicitly configured network service, and provide routing, filtering, NAT, and
DNS or DHCP where required. It must also terminate TLS for browser deployments
using HTTPS. Riscbox currently supplies only a local Node test stub; it is not a
production bridge or part of a distribution.
