(function() {var implementors = {};
implementors["tokio_proto"] = [{text:"impl&lt;Kind, P&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/struct.TcpClient.html\" title=\"struct tokio_proto::TcpClient\">TcpClient</a>&lt;Kind, P&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;Kind: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;P: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::tcp_client::TcpClient"]},{text:"impl&lt;Kind, P&gt; !<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/struct.Connect.html\" title=\"struct tokio_proto::Connect\">Connect</a>&lt;Kind, P&gt;",synthetic:true,types:["tokio_proto::tcp_client::Connect"]},{text:"impl&lt;Kind, P&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/struct.TcpServer.html\" title=\"struct tokio_proto::TcpServer\">TcpServer</a>&lt;Kind, P&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;Kind: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;P: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::tcp_server::TcpServer"]},{text:"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/pipeline/struct.Pipeline.html\" title=\"struct tokio_proto::pipeline::Pipeline\">Pipeline</a>",synthetic:true,types:["tokio_proto::simple::pipeline::Pipeline"]},{text:"impl&lt;T, P&gt; !<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/pipeline/struct.ClientService.html\" title=\"struct tokio_proto::pipeline::ClientService\">ClientService</a>&lt;T, P&gt;",synthetic:true,types:["tokio_proto::simple::pipeline::client::ClientService"]},{text:"impl&lt;T, P&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/pipeline/struct.ClientFuture.html\" title=\"struct tokio_proto::pipeline::ClientFuture\">ClientFuture</a>&lt;T, P&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;P as <a class=\"trait\" href=\"tokio_proto/pipeline/trait.ClientProto.html\" title=\"trait tokio_proto::pipeline::ClientProto\">ClientProto</a>&lt;T&gt;&gt;::<a class=\"type\" href=\"tokio_proto/pipeline/trait.ClientProto.html#associatedtype.Response\" title=\"type tokio_proto::pipeline::ClientProto::Response\">Response</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::simple::pipeline::client::ClientFuture"]},{text:"impl <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/multiplex/struct.Multiplex.html\" title=\"struct tokio_proto::multiplex::Multiplex\">Multiplex</a>",synthetic:true,types:["tokio_proto::simple::multiplex::Multiplex"]},{text:"impl&lt;T, P&gt; !<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/multiplex/struct.ClientService.html\" title=\"struct tokio_proto::multiplex::ClientService\">ClientService</a>&lt;T, P&gt;",synthetic:true,types:["tokio_proto::simple::multiplex::client::ClientService"]},{text:"impl&lt;T, P&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/multiplex/struct.ClientFuture.html\" title=\"struct tokio_proto::multiplex::ClientFuture\">ClientFuture</a>&lt;T, P&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;P as <a class=\"trait\" href=\"tokio_proto/multiplex/trait.ClientProto.html\" title=\"trait tokio_proto::multiplex::ClientProto\">ClientProto</a>&lt;T&gt;&gt;::<a class=\"type\" href=\"tokio_proto/multiplex/trait.ClientProto.html#associatedtype.Response\" title=\"type tokio_proto::multiplex::ClientProto::Response\">Response</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::simple::multiplex::client::ClientFuture"]},{text:"impl&lt;T, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/struct.Body.html\" title=\"struct tokio_proto::streaming::Body\">Body</a>&lt;T, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;E: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::body::Body"]},{text:"impl&lt;T, B&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"enum\" href=\"tokio_proto/streaming/enum.Message.html\" title=\"enum tokio_proto::streaming::Message\">Message</a>&lt;T, B&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::message::Message"]},{text:"impl&lt;B&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/pipeline/struct.StreamingPipeline.html\" title=\"struct tokio_proto::streaming::pipeline::StreamingPipeline\">StreamingPipeline</a>&lt;B&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::pipeline::StreamingPipeline"]},{text:"impl&lt;T, B, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"enum\" href=\"tokio_proto/streaming/pipeline/enum.Frame.html\" title=\"enum tokio_proto::streaming::pipeline::Frame\">Frame</a>&lt;T, B, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;E: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::pipeline::frame::Frame"]},{text:"impl&lt;T&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/pipeline/advanced/struct.Pipeline.html\" title=\"struct tokio_proto::streaming::pipeline::advanced::Pipeline\">Pipeline</a>&lt;T&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;T as <a class=\"trait\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html\" title=\"trait tokio_proto::streaming::pipeline::advanced::Dispatch\">Dispatch</a>&gt;::<a class=\"type\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html#associatedtype.BodyIn\" title=\"type tokio_proto::streaming::pipeline::advanced::Dispatch::BodyIn\">BodyIn</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;T as <a class=\"trait\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html\" title=\"trait tokio_proto::streaming::pipeline::advanced::Dispatch\">Dispatch</a>&gt;::<a class=\"type\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html#associatedtype.BodyOut\" title=\"type tokio_proto::streaming::pipeline::advanced::Dispatch::BodyOut\">BodyOut</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;T as <a class=\"trait\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html\" title=\"trait tokio_proto::streaming::pipeline::advanced::Dispatch\">Dispatch</a>&gt;::<a class=\"type\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html#associatedtype.Error\" title=\"type tokio_proto::streaming::pipeline::advanced::Dispatch::Error\">Error</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a> + <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;T as <a class=\"trait\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html\" title=\"trait tokio_proto::streaming::pipeline::advanced::Dispatch\">Dispatch</a>&gt;::<a class=\"type\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html#associatedtype.In\" title=\"type tokio_proto::streaming::pipeline::advanced::Dispatch::In\">In</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;&lt;T as <a class=\"trait\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html\" title=\"trait tokio_proto::streaming::pipeline::advanced::Dispatch\">Dispatch</a>&gt;::<a class=\"type\" href=\"tokio_proto/streaming/pipeline/advanced/trait.Dispatch.html#associatedtype.Stream\" title=\"type tokio_proto::streaming::pipeline::advanced::Dispatch::Stream\">Stream</a>: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::pipeline::advanced::Pipeline"]},{text:"impl&lt;B&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/multiplex/struct.StreamingMultiplex.html\" title=\"struct tokio_proto::streaming::multiplex::StreamingMultiplex\">StreamingMultiplex</a>&lt;B&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::multiplex::StreamingMultiplex"]},{text:"impl&lt;T, B, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"enum\" href=\"tokio_proto/streaming/multiplex/enum.Frame.html\" title=\"enum tokio_proto::streaming::multiplex::Frame\">Frame</a>&lt;T, B, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;E: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::multiplex::frame::Frame"]},{text:"impl&lt;T&gt; !<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/multiplex/advanced/struct.Multiplex.html\" title=\"struct tokio_proto::streaming::multiplex::advanced::Multiplex\">Multiplex</a>&lt;T&gt;",synthetic:true,types:["tokio_proto::streaming::multiplex::advanced::Multiplex"]},{text:"impl&lt;T, B, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/streaming/multiplex/advanced/struct.MultiplexMessage.html\" title=\"struct tokio_proto::streaming::multiplex::advanced::MultiplexMessage\">MultiplexMessage</a>&lt;T, B, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;B: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;E: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::streaming::multiplex::advanced::MultiplexMessage"]},{text:"impl&lt;R, S, E&gt; !<a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/util/client_proxy/struct.ClientProxy.html\" title=\"struct tokio_proto::util::client_proxy::ClientProxy\">ClientProxy</a>&lt;R, S, E&gt;",synthetic:true,types:["tokio_proto::util::client_proxy::ClientProxy"]},{text:"impl&lt;T, E&gt; <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Sync.html\" title=\"trait core::marker::Sync\">Sync</a> for <a class=\"struct\" href=\"tokio_proto/util/client_proxy/struct.Response.html\" title=\"struct tokio_proto::util::client_proxy::Response\">Response</a>&lt;T, E&gt; <span class=\"where fmt-newline\">where<br>&nbsp;&nbsp;&nbsp;&nbsp;E: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,<br>&nbsp;&nbsp;&nbsp;&nbsp;T: <a class=\"trait\" href=\"https://doc.rust-lang.org/nightly/core/marker/trait.Send.html\" title=\"trait core::marker::Send\">Send</a>,&nbsp;</span>",synthetic:true,types:["tokio_proto::util::client_proxy::Response"]},];

            if (window.register_implementors) {
                window.register_implementors(implementors);
            } else {
                window.pending_implementors = implementors;
            }
        
})()