# Shipping

A Parcel is the physical package an Order ships in. A Carrier is the company that
moves a Parcel. A Route is the sequence of stops a Carrier follows. A Manifest lists
the Parcels a Carrier takes on one Route.

## Rules

Every Parcel shall be assigned to exactly one Carrier before it leaves the warehouse.
A Manifest shall list every Parcel on its Route. When a Carrier scans a Parcel, the
system shall record the stop on the Route.
