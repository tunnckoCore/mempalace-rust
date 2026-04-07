use crate::storage::Storage;
use anyhow::Result;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

type RoomAggregate = (
    BTreeSet<String>,
    BTreeSet<String>,
    usize,
    BTreeSet<String>,
    BTreeSet<String>,
    BTreeMap<String, BTreeSet<String>>,
);

#[derive(Debug, Clone, Serialize)]
pub struct GraphNode {
    pub wings: Vec<String>,
    pub halls: Vec<String>,
    pub count: usize,
    pub dates: Vec<String>,
    pub drawer_types: Vec<String>,
    pub wing_halls: BTreeMap<String, Vec<String>>,
}

pub fn build_graph(
    storage: &Storage,
) -> Result<(BTreeMap<String, GraphNode>, Vec<serde_json::Value>)> {
    let drawers = storage.all_drawers()?;
    let mut room_data: BTreeMap<String, RoomAggregate> = BTreeMap::new();
    for drawer in drawers {
        if drawer.room.is_empty() || drawer.room == "general" || drawer.wing.is_empty() {
            continue;
        }
        let entry = room_data.entry(drawer.room.clone()).or_insert_with(|| {
            (
                BTreeSet::new(),
                BTreeSet::new(),
                0,
                BTreeSet::new(),
                BTreeSet::new(),
                BTreeMap::new(),
            )
        });
        entry.0.insert(drawer.wing.clone());
        if let Some(hall) = drawer.hall.clone() {
            if !hall.is_empty() {
                entry.1.insert(hall.clone());
                entry.5.entry(drawer.wing.clone()).or_default().insert(hall);
            }
        }
        if let Some(date) = drawer.date.clone() {
            if !date.is_empty() {
                entry.3.insert(date);
            }
        }
        entry.4.insert(drawer.drawer_type.clone());
        entry.2 += 1;
    }
    let mut nodes = BTreeMap::new();
    let mut edges = Vec::new();
    for (room, (wings, halls, count, dates, drawer_types, wing_halls)) in &room_data {
        let wing_vec: Vec<_> = wings.iter().cloned().collect();
        let hall_vec: Vec<_> = halls.iter().cloned().collect();
        let date_vec: Vec<_> = dates.iter().cloned().collect();
        let wing_hall_map = wing_halls
            .iter()
            .map(|(wing, halls)| (wing.clone(), halls.iter().cloned().collect()))
            .collect();
        nodes.insert(
            room.clone(),
            GraphNode {
                wings: wing_vec.clone(),
                halls: hall_vec.clone(),
                count: *count,
                dates: date_vec
                    .clone()
                    .into_iter()
                    .rev()
                    .take(5)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect(),
                drawer_types: drawer_types.iter().cloned().collect(),
                wing_halls: wing_hall_map,
            },
        );
        if wing_vec.len() >= 2 {
            for i in 0..wing_vec.len() {
                for j in (i + 1)..wing_vec.len() {
                    for hall in &hall_vec {
                        edges.push(serde_json::json!({"room": room, "wing_a": wing_vec[i], "wing_b": wing_vec[j], "hall": hall, "count": count, "drawer_types": drawer_types, "recent": date_vec.last().cloned().unwrap_or_default()}));
                    }
                }
            }
        }
    }
    Ok((nodes, edges))
}

pub fn traverse(storage: &Storage, start_room: &str, max_hops: usize) -> Result<serde_json::Value> {
    let (nodes, _) = build_graph(storage)?;
    let Some(start) = nodes.get(start_room) else {
        let suggestions: Vec<_> = nodes
            .keys()
            .filter(|room| room.contains(start_room) || start_room.contains(room.as_str()))
            .take(5)
            .cloned()
            .collect();
        return Ok(
            serde_json::json!({"error": format!("Room '{}' not found", start_room), "suggestions": suggestions}),
        );
    };
    let mut visited = BTreeSet::new();
    let mut results = vec![
        serde_json::json!({"room": start_room, "wings": start.wings, "halls": start.halls, "count": start.count, "dates": start.dates, "drawer_types": start.drawer_types, "hop": 0}),
    ];
    visited.insert(start_room.to_string());
    let mut frontier = VecDeque::from([(start_room.to_string(), 0usize)]);
    while let Some((current_room, depth)) = frontier.pop_front() {
        if depth >= max_hops {
            continue;
        }
        let current = nodes.get(&current_room).unwrap();
        let current_wings: BTreeSet<_> = current.wings.iter().cloned().collect();
        for (room, data) in &nodes {
            if visited.contains(room) {
                continue;
            }
            let shared: Vec<_> = data
                .wings
                .iter()
                .filter(|w| current_wings.contains(*w))
                .cloned()
                .collect();
            if !shared.is_empty() {
                visited.insert(room.clone());
                results.push(serde_json::json!({"room": room, "wings": data.wings, "halls": data.halls, "count": data.count, "dates": data.dates, "drawer_types": data.drawer_types, "hop": depth + 1, "connected_via": shared}));
                if depth + 1 < max_hops {
                    frontier.push_back((room.clone(), depth + 1));
                }
            }
        }
    }
    Ok(serde_json::Value::Array(results))
}

pub fn find_tunnels(
    storage: &Storage,
    wing_a: Option<&str>,
    wing_b: Option<&str>,
) -> Result<serde_json::Value> {
    let (nodes, _) = build_graph(storage)?;
    let mut tunnels = Vec::new();
    for (room, data) in nodes {
        if data.wings.len() < 2 {
            continue;
        }
        if let Some(wing_a) = wing_a {
            if !data.wings.iter().any(|w| w == wing_a) {
                continue;
            }
        }
        if let Some(wing_b) = wing_b {
            if !data.wings.iter().any(|w| w == wing_b) {
                continue;
            }
        }
        tunnels.push(serde_json::json!({"room": room, "wings": data.wings, "halls": data.halls, "wing_halls": data.wing_halls, "drawer_types": data.drawer_types, "count": data.count, "recent": data.dates.last().cloned().unwrap_or_default()}));
    }
    Ok(serde_json::Value::Array(tunnels))
}

pub fn graph_stats(storage: &Storage) -> Result<serde_json::Value> {
    let (nodes, edges) = build_graph(storage)?;
    let tunnel_rooms = nodes.values().filter(|n| n.wings.len() >= 2).count();
    let mut per_wing: BTreeMap<String, usize> = BTreeMap::new();
    for node in nodes.values() {
        for wing in &node.wings {
            *per_wing.entry(wing.clone()).or_insert(0) += 1;
        }
    }
    let top_tunnels: Vec<_> = nodes
        .iter()
        .filter(|(_, d)| d.wings.len() >= 2)
        .take(10)
        .map(|(room, d)| serde_json::json!({"room": room, "wings": d.wings, "halls": d.halls, "drawer_types": d.drawer_types, "count": d.count, "recent": d.dates.last().cloned().unwrap_or_default()}))
        .collect();
    Ok(
        serde_json::json!({"total_rooms": nodes.len(), "tunnel_rooms": tunnel_rooms, "total_edges": edges.len(), "rooms_per_wing": per_wing, "top_tunnels": top_tunnels}),
    )
}
