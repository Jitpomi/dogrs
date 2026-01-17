# Elite TypeDB Fleet Management Examples

This document demonstrates the power of our refined TypeDB model with real-world queries that showcase why proper entity-relation modeling matters.

## 🎯 The Power Queries

### 1. Find All Available Drivers Who Can Legally Drive VH004 Right Now

```typeql
match 
  $vehicle isa vehicle, has vehicle-id "VH004", has vehicle-status "operational";
  $employee isa employee, has employee-status "available";
  
  # Employee has all required certifications (via inference)
  (eligible-employee: $employee, eligible-vehicle: $vehicle) isa eligible-assignment;
  
  # No compliance violations
  not {
    $assignment isa assignment,
      (assigned-employee: $employee) isa assignment,
      has assignment-status "non-compliant";
  };
get $employee;
```

**Why This Works**: Pure relational reasoning - no string parsing, no application logic.

### 2. Real-Time Compliance Monitoring

```typeql
match 
  $assignment isa assignment,
    (assigned-employee: $employee, assigned-vehicle: $vehicle) isa assignment,
    has assignment-status "non-compliant";
  $employee has employee-name $name;
  $vehicle has vehicle-id $vid;
get $name, $vid;
```

**Result**: Instantly see which drivers are operating vehicles they're not certified for.

### 3. Certification Gap Analysis

```typeql
match 
  $vehicle isa vehicle, has vehicle-id $vid;
  (requiring-vehicle: $vehicle, required-certification: $cert) isa requires-certification;
  $cert has certification-name $cert_name;
  
  # Count employees who DON'T have this certification
  not {
    $employee isa employee;
    (certified-employee: $employee, held-certification: $cert) isa has-certification;
  };
get $vid, $cert_name; count;
```

**Business Value**: Identify training needs across your fleet.

### 4. Assignment History with Temporal Queries

```typeql
match 
  $assignment isa assignment,
    (assigned-employee: $employee, assigned-vehicle: $vehicle) isa assignment,
    has assigned-at $time;
  $employee has employee-name $name;
  $vehicle has vehicle-id $vid;
  $time > 2024-01-14T00:00:00;
get $name, $vid, $time;
```

**Insight**: Track assignment patterns over time - impossible with string attributes.

## 🚀 What Makes This Elite TypeDB

### Before (❌ Attribute-Based)
```typeql
# Fragile string matching
match 
  $employee has certifications $emp_certs;
  $vehicle has required-certification $req_cert;
  $emp_certs contains $req_cert;  # Breaks with "CDL-A,Hazmat" vs "Hazmat,CDL-A"
```

### After (✅ Relation-Based)
```typeql
# Pure inference
match 
  (eligible-employee: $employee, eligible-vehicle: $vehicle) isa eligible-assignment;
```

## 🎯 Business Rules That Just Work

### Automatic Compliance Validation
```typeql
rule assignment_compliance_validation:
when {
    $assignment isa assignment,
        (assigned-employee: $employee, assigned-vehicle: $vehicle) isa assignment,
        has assignment-status "active";
    
    (requiring-vehicle: $vehicle, required-certification: $cert) isa requires-certification;
    
    not {
        (certified-employee: $employee, held-certification: $cert) isa has-certification;
    };
} then {
    $assignment has assignment-status "non-compliant";
};
```

**Result**: Violations are detected automatically, no application code needed.

### Certification Hierarchy (Future Extension)
```typeql
# Easy to add later
rule cdl_hierarchy:
when {
    $employee isa employee;
    (certified-employee: $employee, held-certification: $cdl_a) isa has-certification;
    $cdl_a has certification-name "CDL-A";
    $cdl_b isa certification, has certification-name "CDL-B";
} then {
    (certified-employee: $employee, held-certification: $cdl_b) isa has-certification;
};
```

## 📊 Analytics Queries

### Fleet Utilization by Certification
```typeql
match 
  $cert isa certification, has certification-name $cert_name;
  (certified-employee: $employee, held-certification: $cert) isa has-certification;
  (assigned-employee: $employee, assigned-vehicle: $vehicle) isa assignment,
    has assignment-status "active";
get $cert_name; count;
```

### Compliance Rate by Vehicle Type
```typeql
match 
  $vehicle isa vehicle, has vehicle-type $type;
  $assignment isa assignment,
    (assigned-vehicle: $vehicle) isa assignment,
    has assignment-status $status;
  { $status == "active"; } or { $status == "non-compliant"; };
get $type, $status; count;
```

## 🔥 The TypeDB Advantage

1. **Zero String Parsing**: All logic is relational
2. **Automatic Inference**: Rules derive new facts
3. **Temporal Queries**: Track changes over time
4. **Extensible**: Add new certifications without code changes
5. **Composable**: Combine with existing functions seamlessly

This is what "TypeDB as a systems database" looks like in practice - business logic lives in the data layer, not scattered across application code.
