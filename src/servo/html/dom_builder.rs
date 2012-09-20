#[doc="Constructs a DOM tree from an incoming token stream."]

use au = gfx::geometry;
use au::au;
use dom::base::{Attr, Element, ElementData, ElementKind, HTMLDivElement, HTMLHeadElement,
                   HTMLScriptElement};
use dom::base::{HTMLImageElement, Node, NodeScope, Text, UnknownElement};
use geom::size::Size2D;
use html::lexer;
use html::lexer::Token;
use css::values::Stylesheet;
use vec::{push, push_all_move, flat_map};
use std::net::url::Url;
use resource::resource_task::{ResourceTask, Load, Payload, Done};
use to_str::ToStr;

enum CSSMessage {
    File(Url),
    Exit   
}

enum js_message {
    js_file(Url),
    js_exit
}

#[allow(non_implicitly_copyable_typarams)]
fn link_up_attribute(scope: NodeScope, node: Node, -key: ~str, -value: ~str) {
    // TODO: Implement atoms so that we don't always perform string comparisons.
    scope.read(node, |node_contents| {
        match *node_contents.kind {
          Element(element) => {
            element.attrs.push(~Attr(copy key, copy value));
            match *element.kind {
              HTMLImageElement(img) if key == ~"width" => {
                match int::from_str(value) {
                  None => {
                    // Drop on the floor.
                  }
                  Some(s) => { img.size.width = au::from_px(s); }
                }
              }
              HTMLImageElement(img) if key == ~"height" => {
                match int::from_str(value) {
                  None => {
                    // Drop on the floor.
                  }
                  Some(s) => {
                    img.size.height = au::from_px(s);
                  }
                }
              }
              HTMLDivElement | HTMLImageElement(*) | HTMLHeadElement |
              HTMLScriptElement | UnknownElement => {
                // Drop on the floor.
              }
            }
          }

          _ => {
            fail ~"attempt to link up an attribute to an unstyleable node"
          }
        }
    })
}

fn build_element_kind(tag_name: ~str) -> ~ElementKind {
    match tag_name {
        ~"div" => ~HTMLDivElement,
        ~"img" => {
            ~HTMLImageElement({ mut size: Size2D(au::from_px(100),
                                                 au::from_px(100))
                              })
        }
        ~"script" => ~HTMLScriptElement,
        ~"head" => ~HTMLHeadElement,
        _ => ~UnknownElement 
    }
}

#[doc="Runs a task that coordinates parsing links to css stylesheets.

This function should be spawned in a separate task and spins waiting
for the html builder to find links to css stylesheets and sends off
tasks to parse each link.  When the html process finishes, it notifies
the listener, who then collects the css rules from each task it
spawned, collates them, and sends them to the given result channel.

# Arguments

* `to_parent` - A channel on which to send back the full set of rules.
* `from_parent` - A port on which to receive new links.

"]
fn css_link_listener(to_parent : comm::Chan<Stylesheet>, from_parent : comm::Port<CSSMessage>,
                     resource_task: ResourceTask) {
    let mut result_vec = ~[];

    loop {
        match from_parent.recv() {
          File(url) => {
            let result_port = comm::Port();
            let result_chan = comm::Chan(result_port);
            // TODO: change copy to move once we have match move
            let url = copy url;
            task::spawn(|| {
                // TODO: change copy to move once we can move into closures
                let css_stream = css::lexer::spawn_css_lexer_task(copy url, resource_task);
                let mut css_rules = css::parser::build_stylesheet(css_stream);
                result_chan.send(css_rules);
            });

            push(result_vec, result_port);
          }
          Exit => {
            break;
          }
        }
    }

    let css_rules = flat_map(result_vec, |result_port| { result_port.recv() });
    
    to_parent.send(css_rules);
}

fn js_script_listener(to_parent : comm::Chan<~[~[u8]]>, from_parent : comm::Port<js_message>,
                      resource_task: ResourceTask) {
    let mut result_vec = ~[];

    loop {
        match from_parent.recv() {
          js_file(url) => {
            let result_port = comm::Port();
            let result_chan = comm::Chan(result_port);
            // TODO: change copy to move once we have match move
            let url = copy url;
            do task::spawn || {
                let input_port = Port();
                // TODO: change copy to move once we can move into closures
                resource_task.send(Load(copy url, input_port.chan()));

                let mut buf = ~[];
                loop {
                    match input_port.recv() {
                      Payload(data) => {
                        buf += data;
                      }
                      Done(Ok(*)) => {
                        result_chan.send(buf);
                        break;
                      }
                      Done(Err(*)) => {
                        #error("error loading script %s", url.to_str());
                      }
                    }
                }
            }
            push(result_vec, result_port);
          }
          js_exit => {
            break;
          }  
        }
    }

    let js_scripts = vec::map(result_vec, |result_port| result_port.recv());
    to_parent.send(js_scripts);
}

#[allow(non_implicitly_copyable_typarams)]
fn build_dom(scope: NodeScope, stream: comm::Port<Token>, url: Url,
             resource_task: ResourceTask) -> (Node, comm::Port<Stylesheet>, comm::Port<~[~[u8]]>) {
    // The current reference node.
    let mut cur_node = scope.new_node(Element(ElementData(~"html", ~HTMLDivElement)));
    // We will spawn a separate task to parse any css that is
    // encountered, each link to a stylesheet is sent to the waiting
    // task.  After the html sheet has been fully read, the spawned
    // task will collect the results of all linked style data and send
    // it along the returned port.
    let style_port = comm::Port();
    let child_chan = comm::Chan(style_port);
    let style_chan = task::spawn_listener(|child_port| {
        css_link_listener(child_chan, child_port, resource_task);
    });

    let js_port = comm::Port();
    let child_chan = comm::Chan(js_port);
    let js_chan = task::spawn_listener(|child_port| {
        js_script_listener(child_chan, child_port, resource_task);
    });

    loop {
        let token = stream.recv();
        match token {
          lexer::Eof => { break; }
          lexer::StartOpeningTag(tag_name) => {
            #debug["starting tag %s", tag_name];
            let element_kind = build_element_kind(tag_name);
            let new_node = scope.new_node(Element(ElementData(copy tag_name, element_kind)));
            scope.add_child(cur_node, new_node);
            cur_node = new_node;
          }
          lexer::Attr(key, value) => {
            #debug["attr: %? = %?", key, value];
            link_up_attribute(scope, cur_node, copy key, copy value);
          }
          lexer::EndOpeningTag => {
            #debug("end opening tag");
          }
          // TODO: Fail more gracefully (i.e. according to the HTML5
          //       spec) if we close more tags than we open.
          lexer::SelfCloseTag => {
            //TODO: check for things other than the link tag
            scope.read(cur_node, |n| {
                match *n.kind {
                  Element(elmt) if elmt.tag_name == ~"link" => {
                    match elmt.get_attr(~"rel") {
                      Some(r) if r == ~"stylesheet" => {
                        match elmt.get_attr(~"href") {
                          Some(filename) => {
                            #debug["Linking to a css sheet named: %s", filename];
                            // FIXME: Need to base the new url on the current url
                            let new_url = make_url(filename, Some(copy url));
                            style_chan.send(File(new_url));
                          }
                          None => { /* fall through*/ }
                        }
                      }
                      _ => { /* fall through*/ }
                    }
                  }
                  _ => { /* fall through*/ }
                }                
            });
            cur_node = scope.get_parent(cur_node).get();
          }
          lexer::EndTag(*) => {
            // TODO: Assert that the closing tag has the right name.
            scope.read(cur_node, |n| {
                match *n.kind {
                  Element(elmt) if elmt.tag_name == ~"script" => {
                    match elmt.get_attr(~"src") {
                      Some(filename) => {
                        #debug["Linking to a js script named: %s", filename];
                        let new_url = make_url(filename, Some(copy url));
                        js_chan.send(js_file(new_url));
                      }
                      None => { /* fall through */ }
                    }
                  }
                  _ => { /* fall though */ }
                }
            });
            cur_node = scope.get_parent(cur_node).get();
          }
          lexer::Text(s) if !s.is_whitespace() => {
            let new_node = scope.new_node(Text(copy s));
            scope.add_child(cur_node, new_node);
          }
          lexer::Text(_) => {
            // FIXME: Whitespace should not be ignored.
          }
          lexer::Doctype => {
            // TODO: Do something here...
          }
        }
    }

    style_chan.send(Exit);
    js_chan.send(js_exit);

    return (cur_node, style_port, js_port);
}