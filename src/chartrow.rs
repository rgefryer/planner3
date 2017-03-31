use std::fmt;
use errors::*;
use chartperiod::ChartPeriod;

/// The time cells for a single Gantt row, split into 1/4 day chunks.
#[derive(Debug)]
pub struct ChartRow {

	num_cells: u32,

	/// Cells, as a bit field
	cells: Vec<u8>
}

/// Results from a resource transfer attempt
#[derive(Debug)]
pub struct TransferResult {

	// Earliest and latest cells transferred 
	// in this attempt.  None if no cells
	// transferred.
	pub earliest: Option<u32>,
	pub latest: Option<u32>,

	// The numbers of cells transferred, and which
	// could not be transferred.
	pub transferred: u32,
	pub failed: u32
}

impl TransferResult {
	fn new(to_transfer: u32) -> TransferResult {
		TransferResult {
			earliest: None,
			latest: None,
			transferred: 0,
			failed: to_transfer
		}
	}

	fn to_transfer(&self) -> u32 {
		self.failed
	}

	fn transfer(&mut self, cell: u32) -> Result<()> {
		if self.failed == 0 {
			bail!("Tried to transer too many cells");
		}

		self.failed -= 1;
		self.transferred += 1;

		if let Some(e) = self.earliest {
			if cell < e {
				self.earliest = Some(cell);
			}
		} else {
			self.earliest = Some(cell);
		}

		if let Some(e) = self.latest {
			if cell > e {
				self.latest = Some(cell);
			}
		} else {
			self.latest = Some(cell);
		}
		Ok(())
	}
}

impl fmt::Display for ChartRow {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {

    	let mut output = String::new();
    	for cell in 0..self.num_cells {
    		if self.is_set(cell) {
				output = output + "o";
    		} else {
				output = output + "_";
    		}
    	}

        write!(f, "[{}]", output)
    }
}

impl ChartRow {

	/// Create new row with all cells unallocated
	pub fn new(num_cells: u32) -> ChartRow {
		ChartRow { 
			num_cells: num_cells, 
			cells: Vec::new() 
		}
	}

	/// Return a string describing the weekly numbers
	pub fn get_weekly_summary(&self) -> String {

		let mut output = String::new();
		for count in self.get_weekly_numbers() {
			match count {
				0 => output.push_str("   "),
				_ => output.push_str(&format!("{: >3}", count))
			};
		}
		output
	}

	/// Return a vector of the weekly numbers
	pub fn get_weekly_numbers(&self) -> Vec<u32> {

		let weeks = 1 + ((self.num_cells - 1) / 20);
		let mut output = Vec::new();
		for week in 0..weeks {
			output.push(self.count_range(&ChartPeriod::new(week*20, (week+1)*20).unwrap()));
		}
		output
	}

	/// Set a specific cell
	pub fn set(&mut self, cell: u32) -> Result<()> {
		
		if cell >= self.num_cells {
			bail!(format!("Failed to set cell {}, chart width is {}", cell, self.num_cells));
		}

		let byte = (cell / 8) as usize;
		let bit = cell % 8;
		let test = 0x01 << bit;

		while self.cells.len() <= byte {
			self.cells.push(0);
		}

		self.cells[byte] |= test;

		Ok(())
	}

	/// Unset a specific cell
	pub fn unset(&mut self, cell: u32) -> Result<()> {

		if cell >= self.num_cells {
			bail!(format!("Failed to unset cell {}, chart width is {}", cell, self.num_cells));
		}

		let byte = (cell / 8) as usize;
		let bit = cell % 8;
		let test = 0x01 << bit;

		if self.cells.len() > byte {
			self.cells[byte] &= !test;
		}

		Ok(())
	}

	/// Test whether a specific cell is set
	pub fn is_set(&self, cell: u32) -> bool {
		let byte = (cell / 8) as usize;
		let bit = cell % 8;
		let test = 0x01 << bit;

		if self.cells.len() < byte + 1 {
			return false;
		}

		self.cells[byte] & test == test
	}

	/// Set a range of cells
	pub fn set_range(&mut self, period: &ChartPeriod) -> Result<()> {

		for cell in period.get_first() .. period.get_last() + 1 {
			self.set(cell).chain_err(|| format!("Failed to set period {:?}", period))?;
		}
		Ok(())
	}

	/// Count how many of a range of cells are set
	pub fn count_range(&self, period: &ChartPeriod) -> u32 {

	  	let mut count = 0u32;
		for cell in period.get_first() .. period.get_last() + 1 {
			if self.is_set(cell) {
				count += 1;
			}
		}

		count
	}

	/// Count the number of cells that are set
	pub fn count(&self) -> u32 {
		let mut count = 0u32;
		for cell in &self.cells {
			let mut cell_copy = *cell;
			while cell_copy != 0 {
				if cell_copy & 0x01 == 0x01 {
					count += 1;
				}
				cell_copy >>= 1;
			}
		}
		count
	}

	/// Transfer a number of cells to another row.  The cells are inserted
	/// from the start of the range, as allowed by existing commitments.
	/// Returns a tuple of
	/// - the last cell transferred (Option)
	/// - the number of cells transferred
	/// - the number of cells that could not be transferred
	pub fn fill_transfer_to(&mut self,
					        dest: &mut ChartRow, 
					        count: u32, 
					        period: &ChartPeriod) -> Result<TransferResult> {

		let mut rc = TransferResult::new(count);
		for cell in period.get_first() .. period.get_last() + 1 {
	  		if self.is_set(cell) && !dest.is_set(cell) {
	  			self.unset(cell).chain_err(|| format!("Failed transferring cells from period {:?}", period))?;
	  			dest.set(cell).chain_err(|| format!("Failed transferring cells to period {:?}", period))?;
	  			rc.transfer(cell).chain_err(|| format!("Failed transferring cells in period {:?}", period))?;

	  			if rc.to_transfer() == 0 {
	  				break;
	  			}
	  		}
		}
	  	
		Ok(rc)
	}

	/// Transfer a number of cells to another row.  The cells are inserted
	/// from the end of the range, as allowed by existing commitments.
	/// If not all cells can be transferred, returns an error with the number 
	/// of unallocated cells.  If successful, returns the last cell to be
	/// transferred.
	pub fn reverse_fill_transfer_to(&mut self,
							   dest: &mut ChartRow, 
							   count: u32, 
							   period: &ChartPeriod) -> Result<TransferResult> {


		let mut rc = TransferResult::new(count);
		let mut cell = period.get_last() - 1;
		while cell >= period.get_first() {

	  		if self.is_set(cell) && !dest.is_set(cell) {
	  			self.unset(cell).chain_err(|| format!("Failed transferring cells from period {:?}", period))?;
	  			dest.set(cell).chain_err(|| format!("Failed transferring cells to period {:?}", period))?;
	  			rc.transfer(cell).chain_err(|| format!("Failed transferring cells in period {:?}", period))?;

	  			if rc.to_transfer() == 0 {
	  				break;
	  			}
	  		}
			
			cell -= 1;
		}

		Ok(rc)	  	
	}

	/// Transfer a number of cells to another row.  The cells are smoothed
	/// out over the range, as much as is allowed by existing commitments.
	/// If not all cells can be transferred, returns an error with the number 
	/// of unallocated cells.
	pub fn smear_transfer_to(&mut self,
								dest: &mut ChartRow, 
								count: u32, 
								period: ChartPeriod) -> Result<TransferResult> {

		let mut rc = TransferResult::new(count);
	  	let mut transferred_this_run = 1u32;  // Make sure we do at least one pass

	  	// We have an outer loop, in case the initial smear doesn't complete the job
	  	while transferred_this_run != 0 && rc.to_transfer() != 0 {

		  	let amount_per_cell = rc.to_transfer() as f64 / period.length() as f64;
		  	let mut want_allocated = 0f64; // Num cells that should be allocated by now
		  	transferred_this_run = 0;

		  	// Run through the cells
			for cell in period.get_first() .. period.get_last() + 1 {
		  		want_allocated += amount_per_cell;
		  		if want_allocated > (transferred_this_run as f64) && self.is_set(cell) && !dest.is_set(cell) {

		  			transferred_this_run += 1;
		  			self.unset(cell).chain_err(|| format!("Failed transferring cells from period {:?}", period))?;
		  			dest.set(cell).chain_err(|| format!("Failed transferring cells to period {:?}", period))?;
		  			rc.transfer(cell).chain_err(|| format!("Failed transferring cells in period {:?}", period))?;

		  			if rc.to_transfer() == 0 {
		  				break;
		  			}
		  		}
		  	}
	  	}

		Ok(rc)	  	
	}
}


