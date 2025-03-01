use std::sync::atomic::Ordering;

use scraper::ElementRef;

use super::{super::Item, CID, GLOBAL_DATE};

pub fn parse(item: ElementRef, modal: ElementRef) -> anyhow::Result<Item> {
    let sel_content = scraper::Selector::parse(".content").unwrap();

    let cells = item
        .child_elements()
        .next()
        .ok_or_else(|| anyhow::anyhow!("empty item"))?
        .child_elements()
        .next_chunk::<5>()
        .map_err(|e| anyhow::anyhow!("child error: {e:?}"))?;

    let Some(id) = item.attr("data-service-id").and_then(|x| x.parse().ok()) else {
        anyhow::bail!("id error: {}", cells[0].html());
    };

    let service = cells[1].text().map(str::trim).collect();
    let Some(rate_per_1k) = cells[2]
        .text()
        .map(str::trim)
        .collect::<String>()
        .strip_prefix('$')
        .and_then(|x| x.parse().ok())
    else {
        anyhow::bail!("rate error: {}", cells[1].html());
    };
    let Ok(min_order) = cells[3]
        .text()
        .map(|c| c.replace(char::is_whitespace, ""))
        .collect::<String>()
        .parse()
    else {
        anyhow::bail!("min_order error: {}", cells[3].html());
    };
    let Ok(max_order) = cells[4]
        .text()
        .map(|c| c.replace(char::is_whitespace, ""))
        .collect::<String>()
        .parse()
    else {
        anyhow::bail!("max_order error: {}", cells[4].html());
    };

    let mut description = String::new();
    if let Some(content) = modal.select(&sel_content).next() {
        for node in content.children() {
            match node.value() {
                scraper::Node::Text(text) => description.push_str(text),
                scraper::Node::Element(elem) if elem.name() == "br" => description.push('\n'),
                _ => (),
            }
        }
    }

    Ok(Item {
        id,
        time: *GLOBAL_DATE.read(),
        cid: CID.load(Ordering::SeqCst),
        service,
        rate_per_1k,
        min_order,
        max_order,
        description,
    })
}
