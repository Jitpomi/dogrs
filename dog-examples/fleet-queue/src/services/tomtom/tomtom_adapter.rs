use anyhow::Result;
use serde_json::{Value, json};
use reqwest::Client;
use std::time::Duration;
use dog_core::DogApp;
use crate::services::FleetParams;

/// TomTom adapter that makes direct API calls to TomTom services
pub struct TomTomAdapter {
    client: Client,
    api_key: String,
    base_url: String,
    app: std::sync::Arc<DogApp<Value, FleetParams>>,
}

impl TomTomAdapter {
    pub fn new(app: &DogApp<Value, FleetParams>) -> Result<Self> {
        let api_key = app.get("tomtom.key").ok_or_else(|| anyhow::anyhow!("Missing 'tomtom.key' field"))?;
        let base_url = app.get("tomtom.baseUrl").ok_or_else(|| anyhow::anyhow!("Missing 'tomtom.baseUrl' field"))?;
            
        Ok(Self {
            client: Client::new(),
            api_key,
            base_url,
            app: std::sync::Arc::new(app.clone()),
        })
    }

    /// Handle geocoding requests
    pub async fn geocode(&self, data: Value) -> Result<Value> {
        let address = data.get("address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'address' field"))?;
            
        let delivery_id = data.get("delivery_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'delivery_id' field"))?;

        // Make direct TomTom API call
        let url = format!(
            "{}/search/2/geocode/{}.json?key={}",
            self.base_url,
            urlencoding::encode(address),
            self.api_key
        );

        let timeout_secs = self.app.get("tomtom.geocode.timeout")
            .unwrap_or_else(|| "10".to_string())
            .parse()
            .unwrap_or(10);
            
        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom geocode request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Extract coordinates from TomTom response
        if let Some(results) = json_response["results"].as_array() {
            if let Some(first_result) = results.first() {
                if let Some(position) = first_result["position"].as_object() {
                    let lat = position["lat"].as_f64().unwrap_or(0.0);
                    let lng = position["lon"].as_f64().unwrap_or(0.0);
                    let formatted = first_result["address"]["freeformAddress"]
                        .as_str()
                        .unwrap_or(address);

                    return Ok(json!({
                        "latitude": lat,
                        "longitude": lng,
                        "formatted_address": formatted,
                        "delivery_id": delivery_id,
                        "status": "success"
                    }));
                }
            }
        }

        Err(anyhow::anyhow!("No geocoding results found"))
    }

    /// Handle address search requests for autocomplete
    pub async fn search_addresses(&self, data: Value) -> Result<Value> {
        let query = data.get("query")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'query' field"))?;

        // Make direct TomTom Search API call for autocomplete
        let url = format!(
            "{}/search/2/search/{}.json?key={}&limit=5&typeahead=true",
            self.base_url,
            urlencoding::encode(query),
            self.api_key
        );

        let timeout_secs = self.app.get("tomtom.search.timeout")
            .unwrap_or_else(|| "10".to_string())
            .parse()
            .unwrap_or(10);
            
        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom search request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Return the results array directly for autocomplete
        if let Some(results) = json_response["results"].as_array() {
            return Ok(json!({
                "results": results,
                "status": "success"
            }));
        }

        Err(anyhow::anyhow!("No search results found"))
    }

    /// Handle route calculation requests
    pub async fn calculate_route(&self, data: Value) -> Result<Value> {
        let from_lat = data.get("from_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'from_lat' field"))?;
            
        let from_lng = data.get("from_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'from_lng' field"))?;
            
        let to_lat = data.get("to_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to_lat' field"))?;
            
        let to_lng = data.get("to_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to_lng' field"))?;
            
        let vehicle_id = data.get("vehicle_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
            
        let delivery_id = data.get("delivery_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Extract vehicle parameters for commercial vehicle routing
        let mut url_params = vec![format!("key={}", self.api_key)];
        
        // Try to get vehicle specs from database first, fallback to request params
        if let Ok(vehicle_specs) = self.get_vehicle_specs(vehicle_id).await {
            if let Some(engine_type) = vehicle_specs.get("engine_type").and_then(|v| v.as_str()) {
                url_params.push(format!("vehicleEngineType={}", engine_type));
            }
            if let Some(max_speed) = vehicle_specs.get("max_speed").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleMaxSpeed={}", max_speed as i64));
            }
            if let Some(weight) = vehicle_specs.get("vehicle_weight").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleWeight={}", weight as i64));
            }
            if let Some(axle_weight) = vehicle_specs.get("axle_weight").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleAxleWeight={}", axle_weight as i64));
            }
            if let Some(length) = vehicle_specs.get("vehicle_length").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleLength={}", length));
            }
            if let Some(width) = vehicle_specs.get("vehicle_width").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleWidth={}", width));
            }
            if let Some(height) = vehicle_specs.get("vehicle_height").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleHeight={}", height));
            }
            if let Some(commercial) = vehicle_specs.get("is_commercial").and_then(|v| v.as_bool()) {
                if commercial {
                    url_params.push("vehicleCommercial=true".to_string());
                }
            }
        } else if let Some(vehicle_params) = data.get("vehicleParams") {
            // Fallback to frontend-provided params if database query fails
            if let Some(engine_type) = vehicle_params.get("engineType").and_then(|v| v.as_str()) {
                url_params.push(format!("vehicleEngineType={}", engine_type));
            }
            if let Some(max_speed) = vehicle_params.get("maxSpeed").and_then(|v| v.as_i64()) {
                url_params.push(format!("vehicleMaxSpeed={}", max_speed));
            }
            if let Some(weight) = vehicle_params.get("weight").and_then(|v| v.as_i64()) {
                url_params.push(format!("vehicleWeight={}", weight));
            }
            if let Some(axle_weight) = vehicle_params.get("axleWeight").and_then(|v| v.as_i64()) {
                url_params.push(format!("vehicleAxleWeight={}", axle_weight));
            }
            if let Some(length) = vehicle_params.get("length").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleLength={}", length));
            }
            if let Some(width) = vehicle_params.get("width").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleWidth={}", width));
            }
            if let Some(height) = vehicle_params.get("height").and_then(|v| v.as_f64()) {
                url_params.push(format!("vehicleHeight={}", height));
            }
            if let Some(commercial) = vehicle_params.get("commercial").and_then(|v| v.as_bool()) {
                if commercial {
                    url_params.push("vehicleCommercial=true".to_string());
                }
            }
        }

        // Make direct TomTom Routing API call with commercial vehicle parameters
        let url = format!(
            "{}/routing/1/calculateRoute/{},{}:{},{}/json?{}",
            self.base_url,
            from_lat, from_lng,
            to_lat, to_lng,
            url_params.join("&")
        );

        let timeout_secs = self.app.get("tomtom.route.timeout")
            .unwrap_or_else(|| "15".to_string())
            .parse()
            .unwrap_or(15);

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom route request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Extract route information from TomTom response
        if let Some(routes) = json_response["routes"].as_array() {
            if let Some(route) = routes.first() {
                let summary = &route["summary"];
                let distance = summary["lengthInMeters"].as_i64().unwrap_or(0) as i32;
                let duration = summary["travelTimeInSeconds"].as_i64().unwrap_or(0) as i32;

                return Ok(json!({
                    "distance_meters": distance,
                    "duration_seconds": duration,
                    "vehicle_id": vehicle_id,
                    "delivery_id": delivery_id,
                    "route": {
                        "from": {"lat": from_lat, "lng": from_lng},
                        "to": {"lat": to_lat, "lng": to_lng}
                    },
                    "status": "success"
                }));
            }
        }

        Err(anyhow::anyhow!("No route found"))
    }

    /// Handle ETA update requests (calculates ETA from current position to destination)
    pub async fn update_eta(&self, data: Value) -> Result<Value> {
        let vehicle_id = data.get("vehicle_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'vehicle_id' field"))?;
            
        let delivery_id = data.get("delivery_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("Missing 'delivery_id' field"))?;
            
        let current_lat = data.get("current_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'current_lat' field"))?;
            
        let current_lng = data.get("current_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'current_lng' field"))?;
            
        let dest_lat = data.get("dest_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'dest_lat' field"))?;
            
        let dest_lng = data.get("dest_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'dest_lng' field"))?;

        // Use TomTom Routing API to calculate ETA
        let url = format!(
            "{}/routing/1/calculateRoute/{},{}:{},{}/json?key={}&computeTravelTimeFor=all",
            self.base_url,
            current_lat, current_lng,
            dest_lat, dest_lng,
            self.api_key
        );

        let timeout_secs = self.app.get("tomtom.eta.timeout")
            .unwrap_or_else(|| "15".to_string())
            .parse()
            .unwrap_or(15);

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom ETA request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Extract ETA information
        if let Some(routes) = json_response["routes"].as_array() {
            if let Some(route) = routes.first() {
                let summary = &route["summary"];
                let travel_time = summary["travelTimeInSeconds"].as_i64().unwrap_or(0);
                let distance = summary["lengthInMeters"].as_i64().unwrap_or(0);
                
                let eta = chrono::Utc::now() + chrono::Duration::seconds(travel_time);

                return Ok(json!({
                    "vehicle_id": vehicle_id,
                    "delivery_id": delivery_id,
                    "current_location": {"lat": current_lat, "lng": current_lng},
                    "destination": {"lat": dest_lat, "lng": dest_lng},
                    "estimated_arrival": eta.to_rfc3339(),
                    "remaining_time_seconds": travel_time,
                    "remaining_distance_meters": distance,
                    "status": "success"
                }));
            }
        }

        Err(anyhow::anyhow!("Could not calculate ETA"))
    }

    /// Handle reverse geocoding requests using TomTom Reverse Geocoding API
    pub async fn reverse_geocode(&self, data: Value) -> Result<Value> {
        let lat = data.get("lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'lat' field"))?;
            
        let lng = data.get("lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'lng' field"))?;

        // Make direct TomTom Reverse Geocoding API call
        let url = format!(
            "{}/search/2/reverseGeocode/{},{}.json?key={}",
            self.base_url,
            lat,
            lng,
            self.api_key
        );

        let timeout_secs = self.app.get("tomtom.reverse_geocode.timeout")
            .unwrap_or_else(|| "10".to_string())
            .parse()
            .unwrap_or(10);

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom reverse geocode request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Extract address from TomTom reverse geocoding response
        if let Some(addresses) = json_response["addresses"].as_array() {
            if let Some(first_address) = addresses.first() {
                if let Some(address_obj) = first_address["address"].as_object() {
                    let formatted_address = format!(
                        "{}, {}, {} {}",
                        address_obj.get("streetNumber").and_then(|v| v.as_str()).unwrap_or(""),
                        address_obj.get("streetName").and_then(|v| v.as_str()).unwrap_or(""),
                        address_obj.get("municipality").and_then(|v| v.as_str()).unwrap_or(""),
                        address_obj.get("postalCode").and_then(|v| v.as_str()).unwrap_or("")
                    ).trim_start_matches(", ").trim_end_matches(", ").to_string();
                    
                    return Ok(json!({
                        "status": "success",
                        "address": formatted_address,
                        "lat": lat,
                        "lng": lng
                    }));
                }
            }
        }

        // Fallback if no address found
        Ok(json!({
            "status": "success",
            "address": format!("{:.4}, {:.4}", lat, lng),
            "lat": lat,
            "lng": lng
        }))
    }

    /// Handle traffic check requests using TomTom Traffic Flow API
    pub async fn check_traffic(&self, data: Value) -> Result<Value> {
        let from_lat = data.get("from_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'from_lat' field"))?;
            
        let from_lng = data.get("from_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'from_lng' field"))?;
            
        let to_lat = data.get("to_lat")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to_lat' field"))?;
            
        let to_lng = data.get("to_lng")
            .and_then(|v| v.as_f64())
            .ok_or_else(|| anyhow::anyhow!("Missing 'to_lng' field"))?;
            
        let vehicle_id = data.get("vehicle_id")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        // Use TomTom Routing API with traffic information
        let url = format!(
            "{}/routing/1/calculateRoute/{},{}:{},{}/json?key={}&traffic=true&routeType=fastest",
            self.base_url,
            from_lat, from_lng,
            to_lat, to_lng,
            self.api_key
        );

        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("TomTom traffic request failed: {}", e))?;

        if !response.status().is_success() {
            return Err(anyhow::anyhow!("TomTom API error: {}", response.status()));
        }

        let json_response: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse TomTom response: {}", e))?;

        // Extract traffic information
        if let Some(routes) = json_response["routes"].as_array() {
            if let Some(route) = routes.first() {
                let summary = &route["summary"];
                let travel_time = summary["travelTimeInSeconds"].as_i64().unwrap_or(0);
                let live_traffic_time = summary["liveTrafficIncidentsTravelTimeInSeconds"].as_i64().unwrap_or(0);
                let traffic_delay = live_traffic_time - travel_time;

                let heavy_threshold = self.app.get("tomtom.traffic.heavy_threshold")
                    .unwrap_or_else(|| "600".to_string())
                    .parse()
                    .unwrap_or(600);
                let moderate_threshold = self.app.get("tomtom.traffic.moderate_threshold")
                    .unwrap_or_else(|| "300".to_string())
                    .parse()
                    .unwrap_or(300);
                let congestion_level = if traffic_delay > heavy_threshold { 
                    "heavy" 
                } else if traffic_delay > moderate_threshold { 
                    "moderate" 
                } else { 
                    "light" 
                };

                return Ok(json!({
                    "vehicle_id": vehicle_id,
                    "route": {
                        "from": {"lat": from_lat, "lng": from_lng},
                        "to": {"lat": to_lat, "lng": to_lng}
                    },
                    "travel_time_seconds": travel_time,
                    "traffic_delay_seconds": traffic_delay,
                    "congestion_level": congestion_level,
                    "status": "success"
                }));
            }
        }

        Err(anyhow::anyhow!("Could not get traffic information"))
    }

    /// Get vehicle specifications from database for routing
    async fn get_vehicle_specs(&self, vehicle_id: &str) -> Result<Value> {
        let query = format!(
            r#"
            match $v isa vehicle, has vehicle-id "{}",
                has engine-type $engine,
                has max-speed $speed,
                has vehicle-weight $weight,
                has axle-weight $axle,
                has vehicle-length $length,
                has vehicle-width $width,
                has vehicle-height $height,
                has is-commercial $commercial;
            select $engine, $speed, $weight, $axle, $length, $width, $height, $commercial;
            "#,
            vehicle_id
        );

        let response = self.client
            .post(&format!("{}/operations", self.app.get("api.baseUrl").unwrap_or_else(|| "http://localhost:3036".to_string())))
            .header("Content-Type", "application/json")
            .header("x-service-method", "read")
            .json(&json!({
                "query": query
            }))
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to query vehicle specs: {}", e))?;

        let result: Value = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse vehicle specs response: {}", e))?;

        if let Some(data) = result.get("ok").and_then(|v| v.as_array()) {
            if let Some(first_result) = data.first() {
                return Ok(json!({
                    "engine_type": first_result.get("engine").and_then(|v| v.get("value")),
                    "max_speed": first_result.get("speed").and_then(|v| v.get("value")),
                    "vehicle_weight": first_result.get("weight").and_then(|v| v.get("value")),
                    "axle_weight": first_result.get("axle").and_then(|v| v.get("value")),
                    "vehicle_length": first_result.get("length").and_then(|v| v.get("value")),
                    "vehicle_width": first_result.get("width").and_then(|v| v.get("value")),
                    "vehicle_height": first_result.get("height").and_then(|v| v.get("value")),
                    "is_commercial": first_result.get("commercial").and_then(|v| v.get("value"))
                }));
            }
        }

        Err(anyhow::anyhow!("Vehicle specs not found for vehicle: {}", vehicle_id))
    }

    /// Get service statistics (simplified since no queue)
    pub async fn get_stats(&self) -> Result<Value> {
        Ok(json!({
            "service": "tomtom",
            "status": "active",
            "api_base_url": self.base_url,
            "available_endpoints": [
                "geocode",
                "route",
                "eta", 
                "traffic"
            ],
            "message": "TomTom service is operational"
        }))
    }
}
