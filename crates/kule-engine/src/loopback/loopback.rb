# Kule: a server that asks for every address listens on this computer only,
# so nothing on the network reaches what runs in a task. Loaded into every
# Ruby process of a task through RUBYOPT. A server given an address of its
# own is left as it is.
require "socket"

module KuleLoopback
  ANY = [nil, "", "0.0.0.0", "::", "*"].freeze

  def self.host(host)
    ANY.include?(host) ? "127.0.0.1" : host
  end

  module Server
    def initialize(host = nil, port = nil, *rest)
      # TCPServer.new(port): every address.
      return super("127.0.0.1", host, *rest) if port.nil?

      super(KuleLoopback.host(host), port, *rest)
    end
  end

  module Sockets
    def tcp_server_sockets(host = nil, port, &block)
      super(KuleLoopback.host(host), port, &block)
    end
  end
end

TCPServer.prepend(KuleLoopback::Server)
Socket.singleton_class.prepend(KuleLoopback::Sockets)
